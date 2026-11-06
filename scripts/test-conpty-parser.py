"""Pure VT parser regression; no process, terminal, user files or desktop."""
import importlib.util
from pathlib import Path
import sys
import unittest

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location(
    'owned_conpty_parser', Path(__file__).with_name('smoke-explorer-conpty.py'))
base = importlib.util.module_from_spec(spec)
spec.loader.exec_module(base)


class ParserTests(unittest.TestCase):
    def test_backspace_moves_without_erasing_and_saturates_at_left_margin(self):
        session = base.Session.__new__(base.Session)
        session.escape = ''
        session.x, session.y = 2, 0
        session.cells = [list('ABC')]
        session.feed('\b')
        self.assertEqual((session.x, session.y), (1, 0))
        self.assertEqual(session.cells, [list('ABC')])
        for _ in range(3):
            session.feed('\b')
        self.assertEqual((session.x, session.y), (0, 0))
        self.assertEqual(session.cells, [list('ABC')])


if __name__ == '__main__':
    unittest.main()
