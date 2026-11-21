import unittest

from tools.vanilla_world_check import log_problems


class VanillaWorldCheckTests(unittest.TestCase):
    def test_ignores_offline_startup_noise_but_not_oracle_failures(self):
        discovery = '[12:00:00] [Minecraft Services Discovery/WARN]: Failed to connect'
        unavailable = 'com.mojang.authlib.exceptions.MinecraftClientException: Services are unavailable'
        startup = [
            discovery,
            'com.mojang.authlib.exceptions.MinecraftClientException: Failed to read from '
            'https://discovery.minecraftservices.com/minecraft/client due to discovery.minecraftservices.com',
            'Caused by: java.net.UnknownHostException: discovery.minecraftservices.com',
            discovery,
            unavailable,
            '[12:00:01] [Server thread/WARN]: Failed to find a usable hardware address from the network interfaces; '
            'using random bytes: 01:23:45:67:89:ab:cd:ef',
        ]
        failures = [
            '[12:00:02] [Server thread/ERROR]: Failed to save chunk',
            unavailable,  # not part of a discovery warning
            'login failed: disconnected',
        ]
        self.assertEqual(log_problems(startup + failures), failures)
