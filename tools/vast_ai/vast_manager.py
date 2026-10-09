#!/usr/bin/env python3
"""
Vast.ai Automation Manager for MacroNest
Automates searching, renting on-demand, fast bootstrap (5090/4090), and instant destroy.
"""
import os
import sys
import json
import subprocess
import argparse
from pathlib import Path

PYTHON_EXE = sys.executable
VAST_CLI = os.path.join(os.path.dirname(PYTHON_EXE), "Scripts", "vastai.exe")
if not os.path.exists(VAST_CLI):
    VAST_CLI = "vastai"

SETUP_SCRIPT_PATH = Path(__file__).parent / "fast_setup.sh"

def run_vast_command(args, capture_json=False):
    cmd = [VAST_CLI] + args
    if capture_json and "--raw" not in cmd:
        cmd.append("--raw")
    try:
        res = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8")
        if res.returncode != 0:
            print(f"[!] Error running vastai: {res.stderr.strip()}", file=sys.stderr)
            return None
        if capture_json:
            try:
                return json.loads(res.stdout)
            except Exception:
                return res.stdout
        return res.stdout
    except FileNotFoundError:
        print("[!] 'vastai' CLI not found. Run: pip install vastai", file=sys.stderr)
        return None

def cmd_init(api_key):
    print(f"[*] Setting Vast.ai API Key...")
    res = subprocess.run([VAST_CLI, "set", "api-key", api_key], capture_output=True, text=True)
    if res.returncode == 0:
        print("[+] API key set successfully!")
    else:
        print(f"[!] Failed to set API key: {res.stderr}")

def cmd_search(gpu="5090", max_price=1.2, min_down=300):
    print(f"[*] Searching for On-Demand {gpu} offers with download speed >= {min_down} Mbps...")
    gpu_filter = f"gpu_name=RTX_{gpu}" if gpu in ["5090", "4090", "3090"] else f"gpu_name={gpu}"
    query = f"{gpu_filter} rent_type=on_demand rentable=true inet_down>={min_down} dph<={max_price}"
    
    offers = run_vast_command(["search", "offers", query, "-o", "dph"], capture_json=True)
    if not offers or not isinstance(offers, list):
        print("[!] No offers found matching query.")
        return

    print(f"\n{'ID':<10} {'GPU':<16} {'Price/Hr':<10} {'DL Speed':<12} {'RAM':<8} {'Disk $/mo':<10} {'Location'}")
    print("-" * 75)
    for o in offers[:10]:
        oid = o.get("id")
        gname = o.get("gpu_name")
        dph = f"${o.get('dph', 0):.3f}"
        down = f"{o.get('inet_down', 0):.0f} Mbps"
        ram = f"{o.get('cpu_ram', 0)//1024} GB"
        disk_cost = f"${o.get('storage_cost', 0)*30*50:.2f}"
        loc = o.get("geolocation", "Unknown")
        print(f"{oid:<10} {gname:<16} {dph:<10} {down:<12} {ram:<8} {disk_cost:<10} {loc}")
    print("\nTo rent an offer, run: python vast_manager.py rent <ID>")

def cmd_rent(offer_id, disk=60):
    if not SETUP_SCRIPT_PATH.exists():
        print(f"[!] Missing setup script at {SETUP_SCRIPT_PATH}")
        return

    script_content = SETUP_SCRIPT_PATH.read_text(encoding="utf-8")
    # Base64 encode the startup script to pass safely into onstart-cmd
    import base64
    b64_script = base64.b64encode(script_content.encode('utf-8')).decode('ascii')
    onstart_cmd = f"echo '{b64_script}' | base64 -d > /root/fast_setup.sh && bash /root/fast_setup.sh"

    image = "pytorch/pytorch:2.6.0-cuda12.6-cudnn9-devel"
    print(f"[*] Renting instance {offer_id} (Disk: {disk}GB, Image: {image})...")
    
    args = [
        "create", "instance", str(offer_id),
        "--image", image,
        "--disk", str(disk),
        "--onstart-cmd", onstart_cmd
    ]
    
    out = run_vast_command(args)
    if out:
        print("[+] Instance creation request sent:")
        print(out)
        print("\n[*] Monitor progress using: python vast_manager.py logs <INSTANCE_ID>")
        print("[*] When done using, DESTROY immediately to save money: python vast_manager.py destroy <INSTANCE_ID>")

def cmd_list():
    instances = run_vast_command(["show", "instances"], capture_json=True)
    if not instances or not isinstance(instances, list):
        print("[*] No active instances found.")
        return

    print(f"\n{'ID':<10} {'Status':<12} {'GPU':<16} {'Cost/Hr':<10} {'SSH Address'}")
    print("-" * 65)
    for inst in instances:
        iid = inst.get("id")
        stat = inst.get("actual_status", "unknown")
        gpu = inst.get("gpu_name", "")
        cost = f"${inst.get('dph_total', 0):.3f}"
        ssh_host = inst.get("ssh_host", "")
        ssh_port = inst.get("ssh_port", "")
        ssh = f"{ssh_host}:{ssh_port}" if ssh_host else "Waiting..."
        print(f"{iid:<10} {stat:<12} {gpu:<16} {cost:<10} {ssh}")

def cmd_logs(instance_id):
    print(f"[*] Fetching logs for instance {instance_id}...")
    out = run_vast_command(["logs", str(instance_id)])
    if out:
        print(out)

def cmd_destroy(instance_id):
    confirm = input(f"[?] Are you sure you want to DESTROY instance {instance_id}? (y/N): ").strip().lower()
    if confirm == "y":
        print(f"[*] Destroying instance {instance_id}...")
        out = run_vast_command(["destroy", "instance", str(instance_id)])
        print("[+] Instance destroyed! No storage fees will be charged.")
        if out:
            print(out)
    else:
        print("[*] Cancelled.")

def main():
    parser = argparse.ArgumentParser(description="Vast.ai Fast Automation Manager for MacroNest")
    subparsers = parser.add_subparsers(dest="cmd", required=True)

    # Init
    p_init = subparsers.add_parser("init", help="Save your Vast.ai API Key")
    p_init.add_argument("api_key", help="Vast.ai API Key from cloud.vast.ai/account/")

    # Search
    p_search = subparsers.add_parser("search", help="Search best on-demand GPU offers")
    p_search.add_argument("--gpu", default="5090", choices=["5090", "4090", "3090"], help="GPU model")
    p_search.add_argument("--max-price", type=float, default=1.2, help="Max price per hour")
    p_search.add_argument("--min-down", type=int, default=300, help="Minimum download speed in Mbps")

    # Rent
    p_rent = subparsers.add_parser("rent", help="Rent instance with 5-minute automated setup")
    p_rent.add_argument("offer_id", type=int, help="Offer ID from search")
    p_rent.add_argument("--disk", type=int, default=60, help="Disk size in GB (Default: 60GB for 23GB model)")

    # List
    subparsers.add_parser("list", help="List active instances")

    # Logs
    p_logs = subparsers.add_parser("logs", help="Get instance startup & tunnel logs")
    p_logs.add_argument("instance_id", type=int, help="Instance ID")

    # Destroy
    p_destroy = subparsers.add_parser("destroy", help="Destroy instance to stop billing")
    p_destroy.add_argument("instance_id", type=int, help="Instance ID")

    args = parser.parse_args()

    if args.cmd == "init":
        cmd_init(args.api_key)
    elif args.cmd == "search":
        cmd_search(gpu=args.gpu, max_price=args.max_price, min_down=args.min_down)
    elif args.cmd == "rent":
        cmd_rent(args.offer_id, disk=args.disk)
    elif args.cmd == "list":
        cmd_list()
    elif args.cmd == "logs":
        cmd_logs(args.instance_id)
    elif args.cmd == "destroy":
        cmd_destroy(args.instance_id)

if __name__ == "__main__":
    main()
