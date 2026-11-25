// Minimal static x86_64 Linux perf_event_open hardware-watchpoint helper for MacroNest.
// Supports both native x86_64 Android games and ARM64 (libhoudini JIT) games without ptrace/TracerPid.

typedef unsigned long long u64;
typedef long long i64;
typedef unsigned int u32;
typedef int i32;
typedef unsigned short u16;
typedef unsigned char u8;

#define AT_FDCWD (-100)
#define O_RDONLY 0
#define PROT_READ 1
#define PROT_WRITE 2
#define MAP_SHARED 1
#define MAP_PRIVATE 2
#define MAP_ANONYMOUS 0x20
#define POLLIN 1
#define POLLHUP 0x10
#define POLLERR 0x08

#define SYS_read 0
#define SYS_write 1
#define SYS_close 3
#define SYS_poll 7
#define SYS_mmap 9
#define SYS_munmap 11
#define SYS_nanosleep 35
#define SYS_exit 60
#define SYS_setrlimit 160
#define SYS_getdents64 217
#define SYS_openat 257
#define SYS_perf_event_open 298
#define SYS_process_vm_readv 310

struct perf_event_attr {
    u32 type;
    u32 size;
    u64 config;
    u64 sample_period;
    u64 sample_type;
    u64 read_format;
    u64 flags;
    u32 wakeup_events;
    u32 bp_type;
    u64 bp_addr;
    u64 bp_len;
    u64 branch_sample_type;
    u64 sample_regs_user;
    u32 sample_stack_user;
    i32 clockid;
    u64 sample_regs_intr;
    u32 aux_watermark;
    u16 sample_max_stack;
    u16 reserved_2;
};

struct iovec { void *iov_base; u64 iov_len; };
struct linux_dirent64 { u64 d_ino; i64 d_off; u16 d_reclen; u8 d_type; char d_name[1]; };
struct rlimit { u64 cur; u64 max; };

static inline i64 sc1(i64 n, i64 a) {
    i64 r; __asm__ volatile("syscall":"=a"(r):"a"(n),"D"(a):"rcx","r11","memory"); return r;
}
static inline i64 sc2(i64 n, i64 a, i64 b) {
    i64 r; __asm__ volatile("syscall":"=a"(r):"a"(n),"D"(a),"S"(b):"rcx","r11","memory"); return r;
}
static inline i64 sc3(i64 n, i64 a, i64 b, i64 c) {
    i64 r; __asm__ volatile("syscall":"=a"(r):"a"(n),"D"(a),"S"(b),"d"(c):"rcx","r11","memory"); return r;
}
static inline i64 sc4(i64 n, i64 a, i64 b, i64 c, i64 d) {
    i64 r; register i64 r10 __asm__("r10")=d;
    __asm__ volatile("syscall":"=a"(r):"a"(n),"D"(a),"S"(b),"d"(c),"r"(r10):"rcx","r11","memory"); return r;
}
static inline i64 sc5(i64 n, i64 a, i64 b, i64 c, i64 d, i64 e) {
    i64 r; register i64 r10 __asm__("r10")=d; register i64 r8 __asm__("r8")=e;
    __asm__ volatile("syscall":"=a"(r):"a"(n),"D"(a),"S"(b),"d"(c),"r"(r10),"r"(r8):"rcx","r11","memory"); return r;
}
static inline i64 sc6(i64 n, i64 a, i64 b, i64 c, i64 d, i64 e, i64 f) {
    i64 r; register i64 r10 __asm__("r10")=d; register i64 r8 __asm__("r8")=e; register i64 r9 __asm__("r9")=f;
    __asm__ volatile("syscall":"=a"(r):"a"(n),"D"(a),"S"(b),"d"(c),"r"(r10),"r"(r8),"r"(r9):"rcx","r11","memory"); return r;
}

void *memset(void *d, int c, u64 n) { u8 *p=d; while(n--)*p++=(u8)c; return d; }

static int streq(const char *a, const char *b) {
    while (*a && *a == *b) { a++; b++; }
    return *a == 0 && *b == 0;
}

static void write_all(const char *s, int len) {
    int off = 0;
    while (off < len) {
        i64 w = sc3(SYS_write, 1, (i64)(s + off), len - off);
        if (w <= 0) break;
        off += (int)w;
    }
}

static void writes(const char *s) {
    int n = 0; while (s[n]) n++;
    write_all(s, n);
}

static int fmt_hex(char *out, u64 v) {
    static const char hx[] = "0123456789abcdef";
    out[0] = '0'; out[1] = 'x';
    int started = 0, p = 2;
    for (int s = 60; s >= 0; s -= 4) {
        int d = (v >> s) & 0xF;
        if (d != 0 || started || s == 0) {
            out[p++] = hx[d];
            started = 1;
        }
    }
    return p;
}

static int fmt_u64(char *out, u64 v) {
    char tmp[24]; int n = 0;
    if (v == 0) { out[0] = '0'; return 1; }
    while (v) { tmp[n++] = (char)('0' + (v % 10)); v /= 10; }
    for (int i = 0; i < n; i++) out[i] = tmp[n - 1 - i];
    return n;
}

static u64 parse_u64(const char *s) {
    u64 v = 0;
    if (s[0] == '0' && (s[1] == 'x' || s[1] == 'X')) {
        s += 2;
        while (*s) {
            u64 d = (*s>='0'&&*s<='9')?*s-'0':(*s>='a'&&*s<='f')?*s-'a'+10:(*s>='A'&&*s<='F')?*s-'A'+10:99;
            if (d == 99) break;
            v = (v << 4) | d;
            s++;
        }
    } else {
        while (*s >= '0' && *s <= '9') { v = v * 10 + (*s - '0'); s++; }
    }
    return v;
}

static i64 parse_i64(const char *s) {
    if (*s == '-') return -(i64)parse_u64(s + 1);
    return (i64)parse_u64(s);
}

static int hex_val(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

static int parse_hex_pattern(const char *s, u8 *pat, u8 *mask, int max_len) {
    int n = 0;
    while (*s && n < max_len) {
        while (*s == ' ' || *s == '\t') s++;
        if (!*s) break;
        if (s[0] == '?' && s[1] == '?') {
            pat[n] = 0; mask[n] = 0; n++; s += 2;
        } else {
            int hi = hex_val(s[0]), lo = hex_val(s[1]);
            if (hi < 0 || lo < 0) break;
            pat[n] = (u8)((hi << 4) | lo); mask[n] = 1; n++; s += 2;
        }
    }
    return n;
}

static i64 vm_read(i32 pid, u64 addr, void *buf, u64 len) {
    struct iovec loc = { buf, len };
    struct iovec rem = { (void *)addr, len };
    return sc6(SYS_process_vm_readv, pid, (i64)&loc, 1, (i64)&rem, 1, 0);
}

static int open_proc_file(i32 pid, const char *suffix, int flags) {
    char path[64];
    int p = 0;
    const char *pfx = "/proc/";
    while (*pfx) path[p++] = *pfx++;
    p += fmt_u64(path + p, (u64)pid);
    while (*suffix) path[p++] = *suffix++;
    path[p] = 0;
    return (int)sc4(SYS_openat, AT_FDCWD, (i64)path, flags, 0);
}

struct map_range { u64 start, end; u8 r, w, x; };

static int read_proc_maps(i32 pid, struct map_range *out, int max_ranges) {
    int fd = open_proc_file(pid, "/maps", O_RDONLY);
    if (fd < 0) return 0;
    char buf[16384];
    int carry = 0, count = 0;
    for (;;) {
        i64 nr = sc3(SYS_read, fd, (i64)(buf + carry), sizeof(buf) - 1 - carry);
        if (nr <= 0) break;
        int total = carry + (int)nr;
        buf[total] = 0;
        int line_start = 0;
        for (int i = 0; i < total; i++) {
            if (buf[i] == '\n') {
                buf[i] = 0;
                const char *ln = buf + line_start;
                u64 s = 0, e = 0;
                int k = 0, d;
                while ((d = hex_val(ln[k])) >= 0) { s = (s << 4) | d; k++; }
                if (ln[k] == '-') {
                    k++;
                    while ((d = hex_val(ln[k])) >= 0) { e = (e << 4) | d; k++; }
                    while (ln[k] == ' ') k++;
                    if (ln[k] && ln[k+1] && ln[k+2] && count < max_ranges) {
                        out[count].start = s;
                        out[count].end = e;
                        out[count].r = (ln[k] == 'r');
                        out[count].w = (ln[k+1] == 'w');
                        out[count].x = (ln[k+2] == 'x');
                        count++;
                    }
                }
                line_start = i + 1;
            }
        }
        carry = total - line_start;
        if (carry > 0 && line_start > 0) {
            for (int i = 0; i < carry; i++) buf[i] = buf[line_start + i];
        } else if (carry >= (int)sizeof(buf) - 2) {
            carry = 0;
        }
    }
    sc1(SYS_close, fd);
    return count;
}

// Subcommand: find <pid> <win_off_in_page_hex> <target_off_in_win> <hex_pattern_with_??>
static int cmd_find(i32 pid, u64 win_off, u64 target_delta_in_win, const char *hex_pat) {
    u8 pat[128], mask[128];
    int pat_len = parse_hex_pattern(hex_pat, pat, mask, 128);
    if (pat_len < 16) return 1;

    static struct map_range ranges[8192];
    int n_ranges = read_proc_maps(pid, ranges, 8192);
    static u8 chunk[262144];

    for (int ri = 0; ri < n_ranges; ri++) {
        if (!ranges[ri].r || !ranges[ri].w) continue;
        u64 span = ranges[ri].end - ranges[ri].start;
        if (span < 4096 || span > (512ULL << 20)) continue;
        for (u64 off = 0; off < span; off += sizeof(chunk)) {
            u64 to_read = span - off;
            if (to_read > sizeof(chunk)) to_read = sizeof(chunk);
            if (vm_read(pid, ranges[ri].start + off, chunk, to_read) != (i64)to_read) continue;
            for (u64 pg = 0; pg + 4096 <= to_read; pg += 4096) {
                if (pg + win_off + (u64)pat_len > to_read) continue;
                const u8 *cand = chunk + pg + win_off;
                int ok = 1;
                for (int k = 0; k < pat_len; k++) {
                    if (mask[k] && cand[k] != pat[k]) { ok = 0; break; }
                }
                if (ok) {
                    u64 found = ranges[ri].start + off + pg + win_off + target_delta_in_win;
                    char out[48]; int p = 0;
                    const char *pfx = "FOUND "; while (*pfx) out[p++] = *pfx++;
                    p += fmt_hex(out + p, found);
                    out[p++] = '\n';
                    write_all(out, p);
                    return 0;
                }
            }
        }
    }
    writes("NOTFOUND\n");
    return 0;
}

// Subcommand: aob <pid> <perm: x|w> <pattern>  (w = rwxp JIT, x = any r-x)
static int cmd_aob(i32 pid, char perm, const char *hex_pat) {
    u8 pat[128], mask[128];
    int pat_len = parse_hex_pattern(hex_pat, pat, mask, 128);
    if (pat_len < 4) return 1;

    static struct map_range ranges[8192];
    int n_ranges = read_proc_maps(pid, ranges, 8192);
    static u8 chunk[262144 + 128];

    for (int ri = 0; ri < n_ranges; ri++) {
        if (!ranges[ri].r || !ranges[ri].x) continue;
        if (perm == 'w' && !ranges[ri].w) continue;
        u64 span = ranges[ri].end - ranges[ri].start;
        if (span < (u64)pat_len || span > (256ULL << 20)) continue;
        for (u64 off = 0; off < span; off += 262144) {
            u64 to_read = span - off;
            if (to_read > 262144 + 128) to_read = 262144 + 128;
            if (vm_read(pid, ranges[ri].start + off, chunk, to_read) <= 0) continue;
            u64 limit = (to_read >= (u64)pat_len) ? (to_read - (u64)pat_len + 1) : 0;
            for (u64 i = 0; i < limit; i++) {
                if (mask[0] && chunk[i] != pat[0]) continue;
                int ok = 1;
                for (int k = 1; k < pat_len; k++) {
                    if (mask[k] && chunk[i + k] != pat[k]) { ok = 0; break; }
                }
                if (ok) {
                    char out[48]; int p = 0;
                    const char *pfx = "AOB "; while (*pfx) out[p++] = *pfx++;
                    p += fmt_hex(out + p, ranges[ri].start + off + i);
                    out[p++] = '\n';
                    write_all(out, p);
                    return 0;
                }
            }
        }
    }
    writes("AOBNONE\n");
    return 0;
}

#define MAX_THREADS 384
#define RING_PAGES 5
#define DATA_BYTES (4 * 4096)

struct thread_watch {
    i32 tid;
    i32 fd;
    u64 ring;
    u64 tail;
};

static struct thread_watch g_watches[MAX_THREADS];
static int g_num_watches = 0;

struct ip_stat { u64 ip; u64 rbp; u64 ctx_pc; u64 count; u8 code[64]; u8 has_code; };
static struct ip_stat g_ip_stats[128];
static int g_num_ip_stats = 0;

struct acc_stat { u64 addr; u64 count; };
static struct acc_stat g_acc_stats[256];
static int g_num_acc_stats = 0;

static int stdin_should_stop(void) {
    struct { i32 fd; short events; short revents; } pfd = { 0, POLLIN, 0 };
    i64 pr = sc3(SYS_poll, (i64)&pfd, 1, 0);
    if (pr > 0) {
        if (pfd.revents & (POLLHUP | POLLERR)) return 1;
        if (pfd.revents & POLLIN) {
            char c = 0;
            i64 r = sc3(SYS_read, 0, (i64)&c, 1);
            if (r <= 0 || c == 'q' || c == 'Q') return 1;
        }
    }
    return 0;
}

static void refresh_threads(i32 pid, const struct perf_event_attr *attr) {
    int dfd = open_proc_file(pid, "/task", O_RDONLY);
    if (dfd < 0) return;

    i32 alive[MAX_THREADS];
    int n_alive = 0;
    char dbuf[4096];
    for (;;) {
        i64 nr = sc3(SYS_getdents64, dfd, (i64)dbuf, sizeof(dbuf));
        if (nr <= 0) break;
        i64 pos = 0;
        while (pos < nr) {
            const struct linux_dirent64 *d = (const struct linux_dirent64 *)(dbuf + pos);
            if (d->d_name[0] >= '0' && d->d_name[0] <= '9' && n_alive < MAX_THREADS) {
                alive[n_alive++] = (i32)parse_u64(d->d_name);
            }
            pos += d->d_reclen;
        }
    }
    sc1(SYS_close, dfd);

    // Close watches whose threads have exited
    for (int i = 0; i < g_num_watches; ) {
        int found = 0;
        for (int j = 0; j < n_alive; j++) {
            if (g_watches[i].tid == alive[j]) { found = 1; break; }
        }
        if (!found) {
            sc2(SYS_munmap, (i64)g_watches[i].ring, RING_PAGES * 4096);
            sc1(SYS_close, g_watches[i].fd);
            g_watches[i] = g_watches[g_num_watches - 1];
            g_num_watches--;
        } else {
            i++;
        }
    }

    // Attach new threads
    for (int j = 0; j < n_alive && g_num_watches < MAX_THREADS; j++) {
        i32 tid = alive[j];
        int exists = 0;
        for (int i = 0; i < g_num_watches; i++) {
            if (g_watches[i].tid == tid) { exists = 1; break; }
        }
        if (exists) continue;
        i64 fd = sc5(SYS_perf_event_open, (i64)attr, tid, -1, -1, 0);
        if (fd < 0) continue;
        i64 ring = sc6(SYS_mmap, 0, RING_PAGES * 4096, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
        if (ring < 0) { sc1(SYS_close, fd); continue; }
        g_watches[g_num_watches].tid = tid;
        g_watches[g_num_watches].fd = (i32)fd;
        g_watches[g_num_watches].ring = (u64)ring;
        g_watches[g_num_watches].tail = 0;
        g_num_watches++;
    }
}

static void ring_copy(const u8 *data, u64 tail, u8 *dst, u64 len) {
    for (u64 i = 0; i < len; i++) {
        dst[i] = data[(tail + i) & (DATA_BYTES - 1)];
    }
}

static void flush_stats(i32 pid, i32 bp_type) {
    static const char hx[] = "0123456789abcdef";
    char line[512];
    if (bp_type == 4) {
        for (int i = 0; i < g_num_acc_stats; i++) {
            if (g_acc_stats[i].count == 0) continue;
            u64 val64 = 0;
            vm_read(pid, g_acc_stats[i].addr, &val64, 8);
            int p = 0;
            const char *pfx = "ACC "; while (*pfx) line[p++] = *pfx++;
            p += fmt_hex(line + p, g_acc_stats[i].addr);
            line[p++] = ' ';
            p += fmt_u64(line + p, g_acc_stats[i].count);
            line[p++] = ' ';
            p += fmt_hex(line + p, val64);
            line[p++] = '\n';
            write_all(line, p);
            g_acc_stats[i].count = 0;
        }
    } else {
        for (int i = 0; i < g_num_ip_stats; i++) {
            if (g_ip_stats[i].count == 0) continue;
            int p = 0;
            const char *pfx = "HIT "; while (*pfx) line[p++] = *pfx++;
            p += fmt_hex(line + p, g_ip_stats[i].ip);
            line[p++] = ' ';
            p += fmt_u64(line + p, g_ip_stats[i].count);
            line[p++] = ' ';
            p += fmt_hex(line + p, g_ip_stats[i].rbp);
            line[p++] = ' ';
            p += fmt_hex(line + p, g_ip_stats[i].ctx_pc);
            line[p++] = ' ';
            for (int k = 0; k < 64; k++) {
                u8 b = g_ip_stats[i].has_code ? g_ip_stats[i].code[k] : 0;
                line[p++] = hx[b >> 4];
                line[p++] = hx[b & 0xF];
            }
            line[p++] = '\n';
            write_all(line, p);
            g_ip_stats[i].count = 0;
        }
    }
}

static int cmd_bp(i32 pid, u64 bp_addr, u32 bp_type, u64 bp_len, i32 base_reg, i32 idx_reg, u64 scale, i64 disp, i32 is_rip) {
    struct rlimit rl = { 8192, 8192 };
    sc2(SYS_setrlimit, 7 /*RLIMIT_NOFILE*/, (i64)&rl);

    struct perf_event_attr attr;
    memset(&attr, 0, sizeof(attr));
    attr.type = 5; // PERF_TYPE_BREAKPOINT
    attr.size = sizeof(attr);
    attr.sample_period = 1;
    // IP(0) | TID(1) | ADDR(3) | REGS_USER(12)
    attr.sample_type = 1ULL | 2ULL | (1ULL << 3) | (1ULL << 12);
    attr.wakeup_events = 1;
    attr.bp_type = bp_type;
    attr.bp_addr = bp_addr;
    attr.bp_len = bp_len;
    attr.flags = (1ULL << 5) | (1ULL << 6); // exclude_kernel | exclude_hv
    attr.sample_regs_user = 0x00FF01FFULL;

    refresh_threads(pid, &attr);
    {
        char msg[64]; int p = 0;
        const char *pfx = "ARMED "; while (*pfx) msg[p++] = *pfx++;
        p += fmt_u64(msg + p, (u64)g_num_watches);
        msg[p++] = '\n';
        write_all(msg, p);
    }
    if (g_num_watches == 0) return 1;

    struct { u64 s, ns; } ts = { 0, 5000000ULL }; // 5ms poll
    u8 rec[512];
    int tick = 0;

    for (;;) {
        if (stdin_should_stop()) break;
        int got_any = 0;

        for (int i = 0; i < g_num_watches; i++) {
            volatile u64 *hdr = (volatile u64 *)g_watches[i].ring;
            u64 head = hdr[1024 / 8];
            __asm__ volatile("lfence" ::: "memory");
            const u8 *data = (const u8 *)(g_watches[i].ring + 4096);

            while (g_watches[i].tail < head) {
                got_any = 1;
                u8 ev_hdr[8];
                ring_copy(data, g_watches[i].tail, ev_hdr, 8);
                u32 ev_type = *(const u32 *)ev_hdr;
                u16 ev_size = *(const u16 *)(ev_hdr + 6);
                if (ev_size == 0 || ev_size > sizeof(rec)) {
                    g_watches[i].tail = head;
                    break;
                }
                if (ev_type == 9 /*PERF_RECORD_SAMPLE*/) {
                    ring_copy(data, g_watches[i].tail, rec, ev_size);
                    const u64 *q = (const u64 *)(rec + 8);
                    u64 ip = q[0];
                    // q[1]=tid, q[2]=addr, q[3]=abi, q[4..20]=regs[0..16]
                    const u64 *regs = q + 4;
                    if (bp_type == 4) {
                        u64 da = 0;
                        if (is_rip) {
                            da = (u64)((i64)bp_addr + disp);
                        } else {
                            u64 b = (base_reg >= 0 && base_reg < 17) ? regs[base_reg] : 0;
                            u64 idx = (idx_reg >= 0 && idx_reg < 17) ? regs[idx_reg] : 0;
                            da = (u64)((i64)b + (i64)(idx * scale) + disp);
                        }
                        if (da != 0) {
                            int found = -1;
                            for (int a = 0; a < g_num_acc_stats; a++) {
                                if (g_acc_stats[a].addr == da) { found = a; break; }
                            }
                            if (found < 0 && g_num_acc_stats < 256) {
                                found = g_num_acc_stats++;
                                g_acc_stats[found].addr = da;
                                g_acc_stats[found].count = 0;
                            }
                            if (found >= 0) g_acc_stats[found].count++;
                        }
                    } else {
                        u64 rbp = regs[6];
                        int found = -1;
                        for (int s = 0; s < g_num_ip_stats; s++) {
                            if (g_ip_stats[s].ip == ip) { found = s; break; }
                        }
                        if (found < 0 && g_num_ip_stats < 128) {
                            found = g_num_ip_stats++;
                            g_ip_stats[found].ip = ip;
                            g_ip_stats[found].rbp = rbp;
                            g_ip_stats[found].ctx_pc = 0;
                            if (rbp > 0x10000) {
                                u64 cpc = 0;
                                if (vm_read(pid, rbp + 0x160, &cpc, 8) == 8 && cpc > 0x10000) {
                                    g_ip_stats[found].ctx_pc = cpc;
                                }
                            }
                            g_ip_stats[found].count = 0;
                            g_ip_stats[found].has_code =
                                (ip >= 48 && vm_read(pid, ip - 48, g_ip_stats[found].code, 64) == 64);
                        }
                        if (found >= 0) g_ip_stats[found].count++;
                    }
                }
                g_watches[i].tail += ev_size;
            }
            hdr[1032 / 8] = g_watches[i].tail;
        }

        if ((tick & 3) == 0) flush_stats(pid, (i32)bp_type); // flush every ~20ms
        if ((tick & 63) == 63) refresh_threads(pid, &attr); // refresh threads every ~320ms
        tick++;
        if (!got_any) sc5(SYS_nanosleep, (i64)&ts, 0, 0, 0, 0);
    }

    flush_stats(pid, (i32)bp_type);
    for (int i = 0; i < g_num_watches; i++) {
        sc2(SYS_munmap, (i64)g_watches[i].ring, RING_PAGES * 4096);
        sc1(SYS_close, g_watches[i].fd);
    }
    return 0;
}

// Subcommand: read <pid> <addr_hex> <len>
static int cmd_read(i32 pid, u64 addr, u64 len) {
    if (len == 0 || len > 512) return 1;
    u8 buf[512];
    if (vm_read(pid, addr, buf, len) != (i64)len) {
        writes("READERR\n");
        return 1;
    }
    static const char hx[] = "0123456789abcdef";
    char out[1040]; int p = 0;
    out[p++] = 'B'; out[p++] = 'Y'; out[p++] = 'T'; out[p++] = 'E'; out[p++] = 'S'; out[p++] = ' ';
    for (u64 i = 0; i < len; i++) { out[p++] = hx[buf[i] >> 4]; out[p++] = hx[buf[i] & 0xF]; }
    out[p++] = '\n';
    write_all(out, p);
    return 0;
}

void entry_main(int argc, char **argv) {
    if (argc < 2) sc1(SYS_exit, 1);
    const char *cmd = argv[1];
    if (streq(cmd, "find") && argc >= 6) {
        i32 pid = (i32)parse_u64(argv[2]);
        u64 win_off = parse_u64(argv[3]);
        u64 delta_in_win = parse_u64(argv[4]);
        sc1(SYS_exit, cmd_find(pid, win_off, delta_in_win, argv[5]));
    } else if (streq(cmd, "aob") && argc >= 5) {
        i32 pid = (i32)parse_u64(argv[2]);
        char perm = argv[3][0];
        sc1(SYS_exit, cmd_aob(pid, perm, argv[4]));
    } else if (streq(cmd, "read") && argc >= 5) {
        i32 pid = (i32)parse_u64(argv[2]);
        u64 addr = parse_u64(argv[3]);
        u64 len = parse_u64(argv[4]);
        sc1(SYS_exit, cmd_read(pid, addr, len));
    } else if (streq(cmd, "bp") && argc >= 6) {
        i32 pid = (i32)parse_u64(argv[2]);
        u64 addr = parse_u64(argv[3]);
        u32 bp_type = (u32)parse_u64(argv[4]);
        u64 bp_len = parse_u64(argv[5]);
        i32 breg = (argc >= 7) ? (i32)parse_i64(argv[6]) : -1;
        i32 ireg = (argc >= 8) ? (i32)parse_i64(argv[7]) : -1;
        u64 scale = (argc >= 9) ? parse_u64(argv[8]) : 1;
        i64 disp = (argc >= 10) ? parse_i64(argv[9]) : 0;
        i32 is_rip = (argc >= 11) ? (i32)parse_i64(argv[10]) : 0;
        sc1(SYS_exit, cmd_bp(pid, addr, bp_type, bp_len, breg, ireg, scale, disp, is_rip));
    }
    sc1(SYS_exit, 1);
}

__asm__(
    ".global _start\n"
    "_start:\n"
    "  mov (%rsp), %rdi\n"
    "  lea 8(%rsp), %rsi\n"
    "  and $-16, %rsp\n"
    "  call entry_main\n"
);
