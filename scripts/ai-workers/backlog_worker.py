#!/usr/bin/env python3
"""Continuously run the rbxport backlog worker and its tmux watcher.

Run with no arguments for the menu. The menu starts two tmux sessions:
  - the worker session runs the Claude Code take-tickets goal
  - the watcher session monitors the worker and restarts it when it finishes

``--watch`` runs the watcher in the foreground. ``--run`` is retained as an
alias for compatibility with older launchd or shell invocations.
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

SESSION = os.environ.get("RBX_SESSION", "rbx backlog worker")
WATCHER_SESSION = os.environ.get("RBX_WATCHER_SESSION", f"{SESSION} watcher")
WATCH_INTERVAL = float(os.environ.get("RBX_WATCH_INTERVAL", "5"))
REPO_DIR = Path(__file__).resolve().parents[2]
SCRIPT_PATH = Path(__file__).resolve()
GOAL = (
    "/goal Continuously run the take-tickets skill. If it stalls, restart it on "
    "different tickets until everything is stalled or there are no tickets left."
)
# Text Claude Code prints in the pane when /goal finishes one way or the other.
DONE_PATTERN = re.compile(r"Goal achieved|Goal could not be achieved|Goal cleared")

# `=` makes tmux match session names exactly rather than by prefix. Pane targets
# need the trailing colon.
SESSION_TARGET = f"={SESSION}"
PANE_TARGET = f"={SESSION}:"
WATCHER_TARGET = f"={WATCHER_SESSION}"


def log(message: str) -> None:
    print(f"[{datetime.now():%Y-%m-%d %H:%M:%S}] {message}", flush=True)


def tmux(*args: str, check: bool = False) -> subprocess.CompletedProcess[str]:
    return subprocess.run(["tmux", *args], capture_output=True, text=True, check=check)


def tmux_session_exists(target: str) -> bool:
    return tmux("has-session", "-t", target).returncode == 0


def has_worker() -> bool:
    return tmux_session_exists(SESSION_TARGET)


def has_watcher() -> bool:
    return tmux_session_exists(WATCHER_TARGET)


def start_worker() -> None:
    log(f'starting worker tmux session "{SESSION}"')
    tmux(
        "new-session",
        "-d",
        "-s",
        SESSION,
        "-c",
        str(REPO_DIR),
        f"cy {shlex.quote(GOAL)}",
        check=True,
    )


def start_watcher() -> None:
    log(f'starting watcher tmux session "{WATCHER_SESSION}"')
    command = shlex.join([sys.executable, str(SCRIPT_PATH), "--watch"])
    tmux(
        "new-session",
        "-d",
        "-s",
        WATCHER_SESSION,
        "-c",
        str(REPO_DIR),
        command,
        check=True,
    )


def kill_worker() -> None:
    if has_worker():
        tmux("kill-session", "-t", SESSION_TARGET, check=True)


def kill_watcher() -> None:
    if has_watcher():
        tmux("kill-session", "-t", WATCHER_TARGET, check=True)


def start_all() -> None:
    if has_worker():
        log(f'worker session "{SESSION}" already exists; keeping it')
    else:
        start_worker()

    if has_watcher():
        log(f'watcher session "{WATCHER_SESSION}" already exists; keeping it')
    else:
        start_watcher()


def stop_all() -> None:
    # Stop the watcher first so it cannot recreate the worker while stopping.
    kill_watcher()
    kill_worker()


def pane_text() -> str:
    return tmux("capture-pane", "-t", PANE_TARGET, "-p", "-S", "-").stdout


def worker_finished() -> bool:
    """Return whether the goal completed or the worker disappeared."""
    if not has_worker():
        return True
    panes = tmux(
        "list-panes", "-t", PANE_TARGET, "-F", "#{pane_dead} #{pane_current_command}"
    )
    if panes.returncode != 0:
        return True
    fields = panes.stdout.split(maxsplit=1)
    if fields and fields[0] == "1":
        return True
    return bool(DONE_PATTERN.search(pane_text()))


def service_status() -> str:
    worker = "running" if has_worker() else "stopped"
    watcher = "running" if has_watcher() else "stopped"
    if worker == watcher:
        return worker
    return f"worker {worker}, watcher {watcher}"


def restart_worker() -> None:
    kill_worker()
    start_worker()


def run_watcher() -> None:
    """Continuously monitor the worker and restart it as soon as it finishes."""
    if WATCH_INTERVAL <= 0:
        raise ValueError("RBX_WATCH_INTERVAL must be greater than zero")

    if has_worker():
        log(f'adopting worker session "{SESSION}"')
    else:
        start_worker()
    log(f"watching worker output every {WATCH_INTERVAL:g}s")

    try:
        while True:
            if worker_finished():
                log("worker goal finished (or session exited); restarting")
                restart_worker()
            time.sleep(WATCH_INTERVAL)
    except KeyboardInterrupt:
        print()
        log("watcher stopped; worker left running")


def instructions() -> str:
    return (
        f'Worker session: "{SESSION}"\n'
        f'Watcher session: "{WATCHER_SESSION}"\n'
        f'  Attach read-write:         tmux attach -t "{SESSION}"\n'
        f'  Attach read-only:          tmux attach -r -t "{SESSION}"\n'
        "  Detach (keep it running):  press Ctrl-b, then d\n"
        f'  Watch supervisor logs:     tmux attach -r -t "{WATCHER_SESSION}"\n'
        f'  Stop both:                 {shlex.quote(str(SCRIPT_PATH))} --stop'
    )


def attach(read_only: bool = False) -> None:
    if not has_worker():
        print("No worker session is running; start both first.")
        return
    mode = "read-only (keystrokes ignored)" if read_only else "read-write"
    print(f"Attaching to the worker, {mode}. Detach with Ctrl-b then d.")
    flags = ["-r"] if read_only else []
    subprocess.run(["tmux", "attach", *flags, "-t", SESSION_TARGET], check=False)


def peek() -> None:
    if not has_worker():
        print("No worker session is running.")
        return
    lines = pane_text().rstrip().splitlines()
    print("\n".join(lines[-30:]) or "(pane is empty)")


def confirm(question: str) -> bool:
    return input(f"{question} [y/N] ").strip().lower() in ("y", "yes")


def menu() -> None:
    def do_start() -> None:
        start_all()
        print("Worker and watcher are running.")

    def do_stop() -> None:
        if not has_worker() and not has_watcher():
            print("Worker and watcher are already stopped.")
        elif confirm("Stop both the worker and watcher?"):
            stop_all()
            print("Worker and watcher stopped.")

    def do_restart() -> None:
        if not has_worker() or confirm("Discard and restart the worker session now?"):
            restart_worker()

    actions = {
        "1": ("Start worker + watcher", do_start),
        "2": ("Stop worker + watcher", do_stop),
        "3": ("Attach to worker (read-write)", attach),
        "4": ("Attach to worker (read-only)", lambda: attach(read_only=True)),
        "5": ("Peek at recent worker output", peek),
        "6": ("Restart worker now", do_restart),
        "7": ("Show attach / detach instructions", lambda: print(instructions())),
        "q": ("Quit", None),
    }
    while True:
        print(f"\n=== rbx backlog worker — {service_status()} ===")
        for key, (label, _) in actions.items():
            print(f"  {key}) {label}")
        choice = input("> ").strip().lower()
        if choice == "q":
            return
        if not choice:
            continue
        entry = actions.get(choice)
        if entry is None:
            print("Unknown choice.")
            continue
        try:
            action = entry[1]
            if action is not None:
                action()
        except subprocess.CalledProcessError as error:
            print(f"tmux failed: {error.stderr or error}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument(
        "--watch",
        action="store_true",
        help="continuously watch and restart the worker in the foreground",
    )
    modes.add_argument(
        "--run",
        action="store_true",
        help="alias for --watch (retained for compatibility)",
    )
    modes.add_argument(
        "--stop",
        action="store_true",
        help="stop both the watcher and worker without opening the menu",
    )
    args = parser.parse_args()
    for tool in ("tmux", "cy"):
        if shutil.which(tool) is None:
            print(f"{tool} not found on PATH", file=sys.stderr)
            return 1
    try:
        if args.stop:
            stop_all()
        elif args.watch or args.run:
            run_watcher()
        else:
            menu()
    except (KeyboardInterrupt, EOFError):
        print()
    except ValueError as error:
        print(error, file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
