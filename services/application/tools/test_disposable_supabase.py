"""Exercise private subprocess diagnostics without creating any Docker resources."""
import os
from pathlib import Path
import signal
import stat
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import Mock, patch

from disposable_supabase import DisposableSupabase, QualificationCommandError


class CommandDiagnostics(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        # Only command's state is needed. Fixture construction would allocate a
        # Supabase project directory and install its Docker wrapper.
        self.realm = object.__new__(DisposableSupabase)
        self.realm.stage = Path(self.temporary.name)
        self.realm.sequence = 0
        self.realm.environment = dict(os.environ)
        self.realm.real_docker = 'unused-docker'

    def private_log(self) -> bytes:
        path = self.realm.stage / 'private-command-1.log'
        self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
        return path.read_bytes()

    def test_success_returns_stdout_and_retains_both_streams(self) -> None:
        output = self.realm.command([sys.executable, '-c',
            'import sys; print("out"); print("err", file=sys.stderr)'])
        self.assertEqual(output, b'out\n')
        self.assertEqual(self.private_log(), b'out\nerr\n')

    def test_nonzero_exit_keeps_diagnostics_out_of_exception(self) -> None:
        with self.assertRaises(QualificationCommandError) as caught:
            self.realm.command([sys.executable, '-c',
                'import sys; print("private-token", file=sys.stderr); sys.exit(7)'])
        self.assertEqual(caught.exception.reason, 'failed')
        self.assertEqual(caught.exception.sequence, 1)
        self.assertNotIn('private-token', str(caught.exception))
        self.assertEqual(self.private_log(), b'private-token\n')

    def test_real_timeout_preserves_output_once_and_reaps_child(self) -> None:
        children: list[subprocess.Popen] = []
        spawn = subprocess.Popen

        def record_child(*args: object, **kwargs: object) -> subprocess.Popen:
            child = spawn(*args, **kwargs)
            children.append(child)
            return child

        with patch('disposable_supabase.subprocess.Popen', side_effect=record_child):
            with self.assertRaises(QualificationCommandError) as caught:
                self.realm.command([sys.executable, '-c',
                    'import sys,time; print("private-out", flush=True); '
                    'print("private-err", file=sys.stderr, flush=True); time.sleep(60)'], timeout=1)
        self.assertEqual(caught.exception.reason, 'timeout')
        self.assertNotIn('private-out', str(caught.exception))
        self.assertEqual(self.private_log(), b'private-out\nprivate-err\n')
        self.assertEqual(len(children), 1)
        self.assertIsNotNone(children[0].poll())

    def test_interruption_retains_output_after_group_termination(self) -> None:
        child = Mock(pid=123, returncode=-signal.SIGTERM)
        child.communicate.side_effect = [KeyboardInterrupt(), (b'partial', b'private')]
        with patch('disposable_supabase.subprocess.Popen', return_value=child), \
             patch('disposable_supabase.os.killpg') as kill:
            with self.assertRaises(QualificationCommandError) as caught:
                self.realm.command(['private-argument'])
        self.assertEqual(caught.exception.reason, 'interrupted')
        self.assertEqual(caught.exception.sequence, 1)
        self.assertNotIn('private-argument', str(caught.exception))
        self.assertEqual([call.args for call in kill.call_args_list],
                         [(123, signal.SIGTERM), (123, signal.SIGKILL)])
        self.assertEqual(self.private_log(), b'partialprivate')

    @unittest.skipUnless(os.name == 'posix', 'qualification uses POSIX process groups')
    def test_timeout_kills_redirected_descendant_after_leader_exits(self) -> None:
        pid_file = self.realm.stage / 'descendant.pid'
        child_code = ('import os,signal,time; from pathlib import Path; '
                      'signal.signal(signal.SIGTERM, signal.SIG_IGN); '
                      f'Path({str(pid_file)!r}).write_text(str(os.getpid())); '
                      'time.sleep(60)')
        leader_code = ('import subprocess,sys,time; '
                       f'subprocess.Popen([sys.executable,"-c",{child_code!r}], '
                       'stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL); '
                       'time.sleep(60)')
        descendant: int | None = None
        try:
            with self.assertRaises(QualificationCommandError):
                self.realm.command([sys.executable, '-c', leader_code], timeout=1)
            descendant = int(pid_file.read_text())
            # An orphan may briefly remain as a dead zombie until its adopter
            # reaps it. kill(pid, 0) alone cannot distinguish that from survival.
            for _ in range(100):
                status = subprocess.run(['ps', '-p', str(descendant), '-o', 'stat='],
                                        capture_output=True, text=True, check=False).stdout.strip()
                if not status or status.startswith('Z'):
                    break
                time.sleep(0.01)
            else:
                self.fail('redirected descendant survived owned process-group termination')
        finally:
            if descendant is None and pid_file.exists():
                descendant = int(pid_file.read_text())
            if descendant is not None:
                try:
                    os.kill(descendant, signal.SIGKILL)
                except ProcessLookupError:
                    pass

    def test_timeout_escalates_to_kill_before_retaining_final_output(self) -> None:
        child = Mock(pid=123, returncode=-signal.SIGKILL)
        child.communicate.side_effect = [
            subprocess.TimeoutExpired('private-argument', 1, output=b'overlap'),
            subprocess.TimeoutExpired('private-argument', 5),
            (b'complete', b'private'),
        ]
        with patch('disposable_supabase.subprocess.Popen', return_value=child), \
             patch('disposable_supabase.os.killpg') as kill:
            with self.assertRaises(QualificationCommandError) as caught:
                self.realm.command(['private-argument'], timeout=1)
        self.assertEqual(caught.exception.reason, 'timeout')
        self.assertEqual([call.args for call in kill.call_args_list],
                         [(123, signal.SIGTERM), (123, signal.SIGKILL)])
        self.assertEqual(child.communicate.call_count, 3)
        self.assertEqual(self.private_log(), b'completeprivate')


if __name__ == '__main__':
    unittest.main()
