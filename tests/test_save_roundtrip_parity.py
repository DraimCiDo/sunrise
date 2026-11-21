import contextlib
import gzip
import io
import struct
import sys
import tempfile
import unittest
from pathlib import Path, PureWindowsPath
from unittest.mock import patch
ROOT = Path(__file__).resolve().parent.parent
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

# The CLI adds tools/ to sys.path; do not let it shadow unittest modules.
with patch.object(sys, 'path', sys.path.copy()):
    from tools import test_save_roundtrip_parity as roundtrip

def tag(kind, name, payload):
    raw = name.encode()
    return bytes([kind]) + struct.pack('>H', len(raw)) + raw + payload


def compound(*tags):
    return b''.join(tags) + b'\0'


def integer(name, value):
    return tag(3, name, struct.pack('>i', value))


class WindowsPath(type(Path())):
    """Keep real file access while relative paths have Windows separators."""
    def relative_to(self, *args, **kwargs):
        return PureWindowsPath(*super().relative_to(*args, **kwargs).parts)

    def __truediv__(self, key):
        return super().__truediv__(PureWindowsPath(key).as_posix())


class SaveRoundtripPathsTests(unittest.TestCase):
    def test_windows_paths_accept_expected_changes_but_retain_real_differences(self):
        with tempfile.TemporaryDirectory() as folder:
            vanilla, sunrise = WindowsPath(folder) / 'vanilla', WindowsPath(folder) / 'sunrise'
            for world, rule, health, slot, score in (
                (vanilla, 24, 20.0, 0, 1),
                (sunrise, 30, 18.0, 1, 2),
            ):
                files = {
                    'level.dat': compound(),
                    'data/minecraft/game_rules.dat': compound(
                        tag(10, 'data', compound(integer('minecraft:max_entity_cramming', rule))),
                    ),
                    'players/data/player.dat': compound(
                        tag(5, 'Health', struct.pack('>f', health)),
                        integer('SelectedItemSlot', slot),
                    ),
                    'data/minecraft/custom.dat': compound(integer('score', score)),
                }
                for rel, payload in files.items():
                    path = world / rel
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(gzip.compress(tag(10, '', payload)))
            # This fixture has no region entities or leash holders.
            with patch.object(roundtrip.scene, 'LEADS', 0):
                report = roundtrip.compare_worlds(vanilla, sunrise)
            self.assertCountEqual(report, [
                ('data/minecraft/custom.dat.score', 1, 2),
                ('players/data/player.dat.Health', ('f', 20.0), ('f', 18.0)),
                ('players/data/player.dat.SelectedItemSlot', 0, 1),
            ])
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(roundtrip.summarize(report), 2)


class SaveRoundtripChunkTests(unittest.TestCase):
    def test_light_only_section_compares_both_layers_and_does_not_hide_blocks(self):
        key = ('r.0.0.mca', 0, 0)
        section = {'Y': ('b', -5), 'BlockLight': ('bytearray', [1] * 2048),
                   'SkyLight': ('bytearray', [2] * 2048)}
        vanilla = {key: {'Status': 'minecraft:full', 'sections': [section]}}
        for other, paths in (
            ([section], []),
            ([], ['BlockLight', 'SkyLight']),
            ([{**section, 'BlockLight': ('bytearray', [0] * 2048),
               'SkyLight': ('bytearray', [0] * 2048)}], ['BlockLight', 'SkyLight']),
            ([{**section, 'block_states': {'palette': [{'Name': 'minecraft:stone'}]},
               'biomes': {'palette': ['minecraft:plains']}}], ['blocks']),
        ):
            with self.subTest(paths=paths):
                report = []
                sunrise = {key: {'Status': 'minecraft:full', 'sections': other}}
                roundtrip.compare_chunks(vanilla, sunrise, report)
                self.assertEqual([path for path, _, _ in report],
                                 [f'chunk.sections.{name}[-5] @{key}' for name in paths])


class SaveRoundtripTimersTests(unittest.TestCase):
    def test_elapsed_despawn_timers_do_not_hide_other_saved_state_changes(self):
        report = [
            ('entity.DespawnDelay @minecraft:trader_llama#57', 47384, 47375),
            ('entity.Passengers[].DespawnDelay @minecraft:trader_llama#58', 100, 91),
            ('entity.PersistenceRequired @minecraft:trader_llama#57', 0, 1),
        ]
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(roundtrip.summarize(report), 1)



class SaveRoundtripLogTests(unittest.TestCase):
    def test_the_containers_missing_network_interface_is_not_a_save_problem(self):
        server = object.__new__(roundtrip.Vanilla)
        server.lines = [
            '[17:01:02] [Server thread/WARN]: Failed to find a usable hardware address from the network interfaces; '
            'using random bytes: 1a:2b:3c:4d:5e:6f:70:81',
            '[17:01:03] [Server thread/WARN]: Failed to load a block entity at 1, 2, 3',
        ]
        self.assertEqual(server.problems(), server.lines[1:])


if __name__ == '__main__':
    unittest.main()
