"""Parser controls for the narrow fixture-only port binding adaptation."""
import unittest
from loopback_docker import bind_arguments


class LoopbackArguments(unittest.TestCase):
    def test_only_publication_flags_change_before_image(self) -> None:
        original = ['create', '--name', 'supabase_db_fixture', '-e', '-p', '--network', 'owned', '--label',
            'com.supabase.cli.project=fixture', '-p', '5432:5432', 'postgres:17', 'sh', '-p', 'command-value']
        actual, event = bind_arguments(original, 'fixture', {'owned'}, 'control')
        expected = original.copy(); expected[10] = '127.0.0.1:5432:5432'
        self.assertEqual(actual, expected)
        self.assertEqual(event['rewrites'], 1)
        self.assertEqual(event['phase'], 'cli')

    def test_supported_mappings_preserve_ports_and_protocol(self) -> None:
        for value, expected in [('5432', '127.0.0.1::5432'), ('1234:5432/udp', '127.0.0.1:1234:5432/udp'),
                               ('127.0.0.1::5432', '127.0.0.1::5432')]:
            args = ['run', '--name', 'control', '--network', 'owned', '--label', 'com.supabase.cli.project=fixture',
                    '--publish', value, 'postgres:17', 'sleep', '1']
            actual, event = bind_arguments(args, 'fixture', {'owned'}, 'control')
            self.assertEqual(actual[8], expected)
            self.assertEqual(event['phase'], 'control')

    def test_ambiguous_flags_mapping_or_ownership_are_rejected(self) -> None:
        base = ['create', '--name', 'supabase_db_fixture', '--network', 'owned', '--label',
                'com.supabase.cli.project=fixture', '-p', '5432:5432', 'postgres:17']
        for index, value in [(2, 'supabase_db_other'), (4, 'other-network'), (6, 'com.supabase.cli.project=other'),
                             (7, '-P'), (7, '-p5432:5432'), (8, '0.0.0.0:5432:5432'), (8, '[::]:5432:5432'),
                             (8, '1-9:1-9'), (8, '--name')]:
            invalid = base.copy(); invalid[index] = value
            with self.assertRaises(ValueError):
                bind_arguments(invalid, 'fixture', {'owned'}, 'control')


if __name__ == '__main__':
    unittest.main()
