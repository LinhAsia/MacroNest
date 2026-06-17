from __future__ import annotations

import json
import re
import subprocess
from collections import Counter
from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "src"
OUTPUT = ROOT / "output" / "dead_code_audit.json"


WARNING_PATTERN = re.compile(
    r"^(?P<path>src\\.+?):(?P<line>\d+):\d+: warning: (?P<message>.+)$"
)
FUNCTION_PATTERN = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?fn\s+(?P<name>[A-Za-z0-9_]+)\s*\(",
    re.MULTILINE,
)


@dataclass
class WarningEntry:
    path: str
    line: int
    message: str

    def to_dict(self) -> dict[str, object]:
        return {"path": self.path, "line": self.line, "message": self.message}


def run_cargo_warning_scan() -> list[WarningEntry]:
    command = [
        "cargo",
        "check",
        "--release",
        "--message-format",
        "short",
    ]
    proc = subprocess.run(
        command,
        cwd=ROOT,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )
    lines = (proc.stdout + "\n" + proc.stderr).splitlines()
    warnings: list[WarningEntry] = []
    for line in lines:
        match = WARNING_PATTERN.match(line.strip())
        if not match:
            continue
        message = match.group("message")
        if not any(
            token in message
            for token in ("never used", "never read", "unreachable", "value assigned")
        ):
            continue
        warnings.append(
            WarningEntry(
                path=match.group("path").replace("\\", "/"),
                line=int(match.group("line")),
                message=message,
            )
        )
    return warnings


def find_backup_files() -> list[str]:
    suffixes = {".bak", ".old", ".tmp", ".orig"}
    files = []
    for path in ROOT.rglob("*"):
        if path.is_file() and path.suffix.lower() in suffixes:
            rel_parts = path.relative_to(ROOT).parts
            if "target" in rel_parts:
                continue
            files.append(str(path.relative_to(ROOT)).replace("\\", "/"))
    return sorted(files)


def count_lines() -> dict[str, int]:
    total_lines = 0
    rs_lines = 0
    rs_files = 0
    for path in SRC.rglob("*.rs"):
        rs_files += 1
        line_count = len(path.read_text(encoding="utf-8", errors="replace").splitlines())
        total_lines += line_count
        rs_lines += line_count
    return {
        "rust_files": rs_files,
        "rust_lines": rs_lines,
        "total_src_lines": total_lines,
    }


def build_occurrence_counter() -> Counter[str]:
    counter: Counter[str] = Counter()
    for path in SRC.rglob("*.rs"):
        text = path.read_text(encoding="utf-8", errors="replace")
        for token in re.findall(r"\b[A-Za-z_][A-Za-z0-9_]*\b", text):
            counter[token] += 1
    return counter


def find_single_reference_functions(counter: Counter[str]) -> list[dict[str, object]]:
    suspects: list[dict[str, object]] = []
    for path in SRC.rglob("*.rs"):
        rel = str(path.relative_to(ROOT)).replace("\\", "/")
        text = path.read_text(encoding="utf-8", errors="replace")
        for match in FUNCTION_PATTERN.finditer(text):
            name = match.group("name")
            if counter[name] != 1:
                continue
            line = text[: match.start()].count("\n") + 1
            suspects.append(
                {
                    "path": rel,
                    "line": line,
                    "name": name,
                    "occurrences": counter[name],
                }
            )
    suspects.sort(key=lambda item: (item["path"], item["line"]))
    return suspects


def summarize_warnings(warnings: list[WarningEntry]) -> dict[str, object]:
    by_kind: Counter[str] = Counter()
    for warning in warnings:
        if "never used" in warning.message:
            by_kind["never_used"] += 1
        elif "never read" in warning.message:
            by_kind["never_read"] += 1
        elif "unreachable" in warning.message:
            by_kind["unreachable"] += 1
        elif "value assigned" in warning.message:
            by_kind["assigned_not_read"] += 1
        else:
            by_kind["other"] += 1
    return {
        "counts": dict(by_kind),
        "top_examples": [warning.to_dict() for warning in warnings[:40]],
    }


def main() -> None:
    warnings = run_cargo_warning_scan()
    counter = build_occurrence_counter()
    report = {
        "summary": count_lines(),
        "backup_files": find_backup_files(),
        "compiler_warnings": summarize_warnings(warnings),
        "single_reference_function_suspects": find_single_reference_functions(counter)[:200],
    }
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(f"Wrote {OUTPUT}")


if __name__ == "__main__":
    main()
