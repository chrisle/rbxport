#!/usr/bin/env python3
"""Menu for the rbxport backlog worker: a Claude Code take-tickets goal in tmux.

Run with no arguments for the menu. `--run` skips the menu and runs the
12-hour scheduler in the foreground (for launchd, ssh, or a spare terminal).

The scheduler checks the tmux session every 12 hours (CHECK_INTERVAL seconds):
  - goal finished (achieved / could not be achieved / cleared) or claude gone:
    kill the session and start a fresh one
  - goal still running: wait another interval
"""
from __future__ import annotations

import argparse
import os
import re
import shlex
import shutil
import subprocess
import sys
import time
from datetime import datetime
from pathlib import Path

SESSION = os.environ.get("SESSION", "rbx backlog worker")
CHECK_INTERVAL = int(os.environ.get("CHECK_INTERVAL", 12 * 3600))
REPO_DIR = Path(__file__).resolve().parents[2]
GOAL = (
    "/goal Continuously run the take-tickets skill. If it stalls, restart it on "
    "different tickets until everything is stalled or there are no tickets left."
)
# Text claude prints in the pane when /goal finishes one way or the other.
DONE_PATTERN = re.compile(r"Goal achieved|Goal could not be achieved|Goal cleared")

# `=` makes tmux match the session name exactly rather than by prefix; pane
# commands need the trailing colon.
SESSION_TARGET = f"={SESSION}"
PANE_TARGET = f"={SESSION}:"


def log(msg: str) -> None:
    print(f"[{datetime.now():%Y-%m-%d %H:%M:%S}] {msg}", flush=True)


def tmux(*args: str, check: bool = False) -> subprocess.CompletedProcess[str]:
    return subprocess.run(["tmux", *args], capture_output=True, text=True, check=check)


def has_session() -> bool:
    return tmux("has-session", "-t", SESSION_TARGET).returncode == 0


def start_session() -> None:
    log(f'starting tmux session "{SESSION}"')
    tmux("new-session", "-d", "-s", SESSION, "-c", str(REPO_DIR),
         f"cy {shlex.quote(GOAL)}", check=True)


def kill_session() -> None:
    if has_session():
        tmux("kill-session", "-t", SESSION_TARGET)


def pane_text() -> str:
    return tmux("capture-pane", "-t", PANE_TARGET, "-p", "-S", "-").stdout


def goal_finished() -> bool:
    """True when the goal is done or the session/claude process is gone."""
    if not has_session():
        return True
    dead = tmux("list-panes", "-t", PANE_TARGET, "-F", "#{pane_dead}").stdout.split()
    if dead and dead[0] == "1":
        return True
    return bool(DONE_PATTERN.search(pane_text()))


def status() -> str:
    if not has_session():
        return "stopped"
    return "finished (will restart on next check)" if goal_finished() else "running"


def restart() -> None:
    kill_session()
    start_session()


def instructions() -> str:
    return (
        f'Session "{SESSION}"\n'
        f'  Attach (monitor):          tmux attach -t "{SESSION}"\n'
        f'  Attach read-only:          tmux attach -r -t "{SESSION}"\n'
        "  Detach (keep it running):  press Ctrl-b, then d\n"
        f'  Peek without attaching:    tmux capture-pane -t "{SESSION}" -p | tail -40\n'
        f'  Stop the worker:           tmux kill-session -t "{SESSION}"'
    )


def run_scheduler() -> None:
    """Foreground loop; Ctrl-C stops it and leaves the tmux session running."""
    if has_session():
        log(f'session "{SESSION}" already exists; adopting it')
    else:
        start_session()
    print(f"\n{instructions()}\n")
    hours = CHECK_INTERVAL / 3600
    print(f"Checking every {hours:g}h. Goal finished => session killed and restarted.")
    print("Ctrl-C stops the scheduler (the tmux session keeps running).\n")
    try:
        while True:
            time.sleep(CHECK_INTERVAL)
            if goal_finished():
                log("goal finished (or session gone); restarting")
                restart()
            else:
                log(f"goal still in progress; waiting another {hours:g}h")
    except KeyboardInterrupt:
        print()
        log("scheduler stopped (tmux session left running)")


def attach(read_only: bool = False) -> None:
    if not has_session():
        print("No session running; start it first.")
        return
    mode = "read-only (keystrokes ignored)" if read_only else "read-write"
    print(f"Attaching, {mode}. Detach with Ctrl-b then d.")
    flags = ["-r"] if read_only else []
    subprocess.run(["tmux", "attach", *flags, "-t", SESSION_TARGET])


def peek() -> None:
    if not has_session():
        print("No session running.")
        return
    lines = pane_text().rstrip().splitlines()
    print("\n".join(lines[-30:]) or "(pane is empty)")


def confirm(question: str) -> bool:
    return input(f"{question} [y/N] ").strip().lower() in ("y", "yes")


def menu() -> None:
    def do_start() -> None:
        if has_session():
            print("Already running.")
        else:
            start_session()

    def do_restart() -> None:
        if not has_session() or confirm("This discards the running session. Restart?"):
            restart()

    def do_stop() -> None:
        if not has_session():
            print("Not running.")
        elif confirm("Kill the running session?"):
            kill_session()
            print("Stopped.")

    actions = {
        "1": ("Start worker", do_start),
        "2": ("Attach to session (read-write)", attach),
        "3": ("Attach read-only (watch live, can't type)", lambda: attach(read_only=True)),
        "4": ("Peek at recent output", peek),
        "5": ("Restart worker now", do_restart),
        "6": ("Stop worker", do_stop),
        "7": ("Run 12h scheduler (foreground, Ctrl-C returns here)", run_scheduler),
        "8": ("Show attach / detach instructions", lambda: print(instructions())),
        "q": ("Quit", None),
    }
    while True:
        print(f"\n=== rbx backlog worker — {status()} ===")
        for key, (label, _) in actions.items():
            print(f"  {key}) {label}")
        choice = input("> ").strip().lower()
        if choice in ("q", ""):
            if choice == "q":
                return
            continue
        entry = actions.get(choice)
        if entry is None:
            print("Unknown choice.")
            continue
        try:
            entry[1]()
        except subprocess.CalledProcessError as e:
            print(f"tmux failed: {e.stderr or e}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--run", action="store_true",
                        help="run the 12-hour scheduler in the foreground, no menu")
    args = parser.parse_args()
    for tool in ("tmux", "cy"):
        if shutil.which(tool) is None:
            print(f"{tool} not found on PATH", file=sys.stderr)
            return 1
    try:
        run_scheduler() if args.run else menu()
    except (KeyboardInterrupt, EOFError):
        print()
    return 0


if __name__ == "__main__":
    sys.exit(main())
