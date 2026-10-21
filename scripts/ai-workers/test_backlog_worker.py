"""Supervisor regressions. Run: python3 -m unittest discover -s scripts/ai-workers."""
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("worker", Path(__file__).with_name("backlog_worker.py"))
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)


class CompletionTests(unittest.TestCase):
    def test_transcript_completion_and_resumed_turn(self):
        session = "12345678-1234-1234-1234-123456789abc"
        def tmux(*args, **kwargs):
            output = "0 bash" if args[0] == "list-panes" else session
            return subprocess.CompletedProcess(args, 0, output, "")
        with tempfile.TemporaryDirectory() as directory, patch.object(worker, "has_worker", return_value=True), patch.object(worker, "tmux", side_effect=tmux), patch.object(worker.Path, "home", return_value=Path(directory)):
            project = worker.re.sub(r"[^a-zA-Z0-9]", "-", str(worker.REPO_DIR))
            transcript = Path(directory) / ".claude/projects" / project / f"{session}.jsonl"
            transcript.parent.mkdir(parents=True)
            done = json.dumps({"type": "system", "subtype": "turn_duration"}) + "\n"
            transcript.write_text(done)
            self.assertTrue(worker.worker_finished())
            transcript.write_text(done + json.dumps({"type": "user"}) + "\n")
            self.assertFalse(worker.worker_finished())
            transcript.write_text(done + '{"type":')
            self.assertFalse(worker.worker_finished())

    def test_legacy_idle_footer(self):
        def tmux(*args, **kwargs):
            return subprocess.CompletedProcess(args, 0, "0 bash" if args[0] == "list-panes" else "", "")
        with patch.object(worker, "has_worker", return_value=True), patch.object(worker, "tmux", side_effect=tmux), patch.object(worker, "pane_text", return_value="✻ Cooked for 31s · done 9:17 AM\n❯ /human-pr-review"):
            self.assertTrue(worker.worker_finished())


class BackoffTests(unittest.TestCase):
    def simulate(self, resume=False):
        clock = [0.0]
        restarts = []
        def sleep(seconds):
            clock[0] += seconds
            if clock[0] > 55:
                raise KeyboardInterrupt
        def restart():
            restarts.append(clock[0])
        with patch.multiple(worker, WATCH_INTERVAL=1, IDLE_GRACE=2, BACKOFF_INITIAL=3, BACKOFF_MAX=12, BACKOFF_RESET_AFTER=100), patch.object(worker, "has_worker", return_value=True), patch.object(worker, "pane_text", return_value="idle"), patch.object(worker, "worker_finished", side_effect=lambda: not resume or clock[0] < 4), patch.object(worker, "restart_worker", side_effect=restart), patch.object(worker.time, "monotonic", side_effect=lambda: clock[0]), patch.object(worker.time, "sleep", side_effect=sleep), patch.object(worker, "log"):
            worker.run_watcher()
        return restarts

    def test_exponential_backoff_caps(self):
        times = self.simulate()
        self.assertEqual(times[:4], [5, 14, 29, 44])

    def test_resuming_during_backoff_cancels_restart(self):
        self.assertEqual(self.simulate(resume=True), [])


if __name__ == "__main__":
    unittest.main()
