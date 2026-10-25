#!/usr/bin/env python3
"""List legacy take-tickets candidates and recheck one before it is claimed.

Only acceptance reports and HOLD are removed here. The take-tickets skill still
applies its other eligibility and cross-machine claim rules.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys

REPO = "chrisle/rbxport"
MARKER = re.compile(r"^\s*\[acceptance test\]", re.IGNORECASE)
TITLE_MARKER = re.compile(r"^\s*(?:rbxport:\s*)?\[acceptance test\]", re.IGNORECASE)
REPORT_HEADER = re.compile(r"^Submitted from rbxport\.\s*\n", re.IGNORECASE)


def label_names(issue: dict) -> set[str]:
    return {
        (label.get("name", "") if isinstance(label, dict) else str(label)).casefold()
        for label in issue.get("labels", [])
    }


def acceptance_report(issue: dict) -> bool:
    if "acceptance-test" in label_names(issue):
        return True
    title = issue.get("title") or ""
    body = issue.get("body") or ""
    if TITLE_MARKER.match(title) or MARKER.match(body):
        return True
    # The report Worker wraps the original description with this header.
    return bool(REPORT_HEADER.match(body) and MARKER.match(REPORT_HEADER.sub("", body, count=1)))


def excluded(issue: dict) -> bool:
    return "hold" in label_names(issue) or acceptance_report(issue)


def gh_json(*args: str):
    result = subprocess.run(["gh", *args], check=True, capture_output=True, text=True)
    return json.loads(result.stdout)


def list_issues() -> list[dict]:
    pages = gh_json("api", f"repos/{REPO}/issues?state=open&per_page=100", "--paginate", "--slurp")
    return [
        issue
        for page in pages
        for issue in page
        if "pull_request" not in issue and not excluded(issue)
    ]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("list", help="print all open, non-acceptance, non-HOLD issues as JSON")
    check = subparsers.add_parser("check", help="recheck an issue just before screening or claiming")
    check.add_argument("number", type=int)
    args = parser.parse_args()
    try:
        if args.command == "list":
            print(json.dumps(list_issues()))
            return 0
        issue = gh_json("issue", "view", str(args.number), "--repo", REPO, "--json", "number,title,body,labels,state")
        if issue.get("state", "").upper() != "OPEN" or excluded(issue):
            print("excluded: closed, HOLD, or acceptance report", file=sys.stderr)
            return 1
        print("eligible for existing skill and claim checks")
        return 0
    except (subprocess.CalledProcessError, json.JSONDecodeError, ValueError) as error:
        print(f"issue intake unavailable: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
