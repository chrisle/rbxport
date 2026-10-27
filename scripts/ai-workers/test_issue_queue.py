"""Focused legacy issue intake decisions; run with unittest discovery."""

import importlib.util
import io
import json
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("issue_queue", Path(__file__).with_name("issue_queue.py"))
queue = importlib.util.module_from_spec(spec)
spec.loader.exec_module(queue)


class IntakeTests(unittest.TestCase):
    @staticmethod
    def api_issue(number, **overrides):
        issue = {
            "number": number,
            "title": f"Issue {number}",
            "body": "A" * 32_000,
            "labels": [{"name": "bug", "color": "ededed"}],
            "assignees": [{"login": "chrisle", "id": 1}],
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-02T00:00:00Z",
            "html_url": f"https://github.com/chrisle/rbxport/issues/{number}",
            "url": f"https://api.github.com/repos/chrisle/rbxport/issues/{number}",
            "extra_metadata": "unused",
        }
        issue.update(overrides)
        return issue

    def test_acceptance_label_and_report_markers(self):
        examples = [
            {"labels": [{"name": "AcCePtAnCe-TeSt"}]},
            {"title": "  [ACCEPTANCE TEST] local check"},
            {"title": "Rbxport: [acceptance test] local check"},
            {"body": " \n [Acceptance Test] local check"},
            {"body": "Submitted from rbxport.\n\n  [AcCePtAnCe TeSt] local check\n\n---\narchive"},
        ]
        for issue in examples:
            with self.subTest(issue=issue):
                self.assertTrue(queue.acceptance_report(issue))
        for issue in ({}, {"body": "Crash after [acceptance test] text"},
                      {"body": "Submitted from rbxport.\n\nCrash mentions [acceptance test]"}):
            with self.subTest(issue=issue):
                self.assertFalse(queue.acceptance_report(issue))

    def test_pagination_excludes_acceptance_and_hold_before_screening(self):
        pages = [[self.api_issue(n) for n in range(1, 101)],
                 [self.api_issue(101, labels=[{"name": "HOLD"}]),
                  self.api_issue(102, pull_request={"url": "pr"})] +
                 [self.api_issue(n) for n in range(103, 206)] +
                 [self.api_issue(206, body="Submitted from rbxport.\n\n[acceptance test]"),
                  self.api_issue(207, labels=[{"name": "acceptance-test"}])]]
        with patch.object(queue, "gh_json", return_value=pages) as gh:
            with patch("sys.argv", ["issue_queue.py", "list"]), redirect_stdout(io.StringIO()) as output:
                self.assertEqual(queue.main(), 0)
        records = json.loads(output.getvalue())
        self.assertEqual([issue["number"] for issue in records], list(range(1, 101)) + list(range(103, 206)))
        self.assertEqual(records[0], {
            "number": 1,
            "title": "Issue 1",
            "labels": [{"name": "bug"}],
            "assignees": [{"login": "chrisle"}],
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-02T00:00:00Z",
            "url": "https://github.com/chrisle/rbxport/issues/1",
        })
        self.assertLess(len(output.getvalue()), 60_000)
        gh.assert_called_once_with("api", "repos/chrisle/rbxport/issues?state=open&per_page=100", "--paginate", "--slurp")

    def test_check_reloads_current_issue_and_fails_closed(self):
        changed_issues = [
            {"state": "OPEN", "labels": [{"name": "acceptance-test"}]},
            {"state": "OPEN", "labels": [{"name": "hOlD"}]},
            {"state": "OPEN", "title": "Rbxport: [acceptance test] report"},
            {"state": "OPEN", "body": "Submitted from rbxport.\n\n[ACCEPTANCE TEST] report"},
            {"state": "CLOSED", "title": "ordinary"},
        ]
        for issue in changed_issues:
            with self.subTest(issue=issue), patch("sys.argv", ["issue_queue.py", "check", "17"]), patch.object(queue, "gh_json", return_value=issue) as gh, redirect_stderr(io.StringIO()):
                self.assertEqual(queue.main(), 1)
                gh.assert_called_once_with("issue", "view", "17", "--repo", queue.REPO, "--json", "number,title,body,labels,state")
        with patch("sys.argv", ["issue_queue.py", "check", "17"]), patch.object(queue, "gh_json", side_effect=subprocess.CalledProcessError(1, "gh")), redirect_stderr(io.StringIO()):
            self.assertEqual(queue.main(), 2)
        with patch("sys.argv", ["issue_queue.py", "check", "17"]), patch.object(queue, "gh_json", return_value={"state": "OPEN", "title": "ordinary"}), redirect_stdout(io.StringIO()):
            self.assertEqual(queue.main(), 0)


if __name__ == "__main__":
    unittest.main()
