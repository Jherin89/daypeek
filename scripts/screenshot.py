#!/usr/bin/env python3
"""Render `today --demo` to docs/screenshot.png (and .html) with fake data only.

Usage: scripts/screenshot.py [--compact]
Needs: a release build (cargo build --release) and Google Chrome/Chromium.
"""
import html, os, re, shutil, subprocess, sys, tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(ROOT, "target", "release", "today")
AT = "2026-10-21T10:58:00-04:00"  # fixed instant => reproducible image

PALETTE = {30: "#45475a", 31: "#f38ba8", 32: "#a6e3a1", 33: "#f9e2af",
           34: "#89b4fa", 35: "#f5c2e7", 36: "#94e2d5", 37: "#bac2de"}
FG, BG = "#cdd6f4", "#1e1e2e"


def ansi_to_html(text):
    out, state = [], {}
    pos = 0
    for m in re.finditer(r"\x1b\[([0-9;]*)m", text):
        out.append(span(text[pos:m.start()], state))
        pos = m.end()
        codes = [int(c) for c in m.group(1).split(";") if c] or [0]
        i = 0
        while i < len(codes):
            c = codes[i]
            if c == 0: state = {}
            elif c == 1: state["b"] = True
            elif c == 2: state["dim"] = True
            elif c == 3: state["i"] = True
            elif c == 9: state["s"] = True
            elif c == 22: state.pop("b", None); state.pop("dim", None)
            elif c in PALETTE: state["fg"] = PALETTE[c]
            elif 90 <= c <= 97: state["fg"] = PALETTE[c - 60]
            elif c == 39: state.pop("fg", None)
            elif c == 38 and codes[i + 1:i + 2] == [2]:
                r, g, b = codes[i + 2:i + 5]; state["fg"] = f"rgb({r},{g},{b})"; i += 4
            i += 1
    out.append(span(text[pos:], state))
    return "".join(out)


def span(s, st):
    if not s:
        return ""
    css = []
    if "fg" in st: css.append(f"color:{st['fg']}")
    if st.get("b"): css.append("font-weight:700")
    if st.get("dim"): css.append("opacity:.55")
    if st.get("i"): css.append("font-style:italic")
    if st.get("s"): css.append("text-decoration:line-through")
    e = html.escape(s)
    return f'<span style="{";".join(css)}">{e}</span>' if css else e


def main():
    compact = "--compact" in sys.argv
    args = [BIN, "--demo", "--color", "always", "--at", AT] + (["--compact"] if compact else [])
    env = dict(os.environ, COLORTERM="truecolor", COLUMNS="84", TZ="America/New_York")
    text = subprocess.run(args, capture_output=True, text=True, env=env, check=True).stdout.rstrip("\n")
    lines = text.split("\n")
    width_px, height_px = 900, 64 + 21 * len(lines) + 40
    page = f"""<!doctype html><meta charset="utf-8"><style>
body{{margin:0;background:transparent}}
.win{{margin:18px;background:{BG};border-radius:12px;box-shadow:0 10px 30px rgba(0,0,0,.35);overflow:hidden}}
.bar{{height:30px;background:#181825;display:flex;align-items:center;padding-left:14px;gap:8px}}
.dot{{width:12px;height:12px;border-radius:50%}}
.title{{color:#6c7086;font:12px -apple-system,Helvetica,sans-serif;margin-left:auto;margin-right:auto;transform:translateX(-30px)}}
pre{{margin:0;padding:8px 18px 18px;color:{FG};font:14px/21px "JetBrains Mono","DejaVu Sans Mono",Menlo,monospace}}
</style><div class="win"><div class="bar"><span class="dot" style="background:#f38ba8"></span>
<span class="dot" style="background:#f9e2af"></span><span class="dot" style="background:#a6e3a1"></span>
<span class="title">~ — today{' --compact' if compact else ''}</span></div>
<pre>{ansi_to_html(text)}</pre></div>"""
    name = "screenshot-compact" if compact else "screenshot"
    docs = os.path.join(ROOT, "docs")
    html_path = os.path.join(docs, name + ".html")
    with open(html_path, "w") as f:
        f.write(page)
    chrome = next((c for c in ["google-chrome", "chromium", "chromium-browser"] if shutil.which(c)), None)
    if not chrome:
        sys.exit(f"wrote {html_path}; install Chrome/Chromium to render the PNG")
    with tempfile.TemporaryDirectory() as profile:
        subprocess.run([chrome, "--headless=new", "--disable-gpu", "--hide-scrollbars", "--no-sandbox",
                        f"--user-data-dir={profile}", "--force-device-scale-factor=2",
                        "--default-background-color=00000000",
                        f"--window-size={width_px},{height_px}",
                        f"--screenshot={os.path.join(docs, name + '.png')}", "file://" + html_path],
                       check=True, capture_output=True)
    print(f"wrote docs/{name}.png")


if __name__ == "__main__":
    main()
