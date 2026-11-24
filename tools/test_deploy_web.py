#!/usr/bin/env python3
"""Offline deployment validation: python3 -m unittest discover -s tools -p 'test_deploy_web.py'."""
import base64
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('deploy_web', Path(__file__).resolve().parents[1] / 'scripts/deploy_web.py')
deploy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(deploy)


class DeploymentTests(unittest.TestCase):
    def test_propagation_wait_requires_eventual_exact_release(self):
        with patch.object(deploy, 'verify_release', side_effect=[ValueError('stale'), None]) as verify, \
                patch.object(deploy.time, 'sleep') as sleep, patch('sys.stdout', io.StringIO()):
            deploy.wait_for_release('https://example.pages.dev/', {})
            self.assertEqual(verify.call_count, 2)
            sleep.assert_called_once_with(3)
        with patch.object(deploy, 'verify_release', side_effect=ValueError('stale')) as verify, \
                patch.object(deploy.time, 'sleep'), patch('sys.stdout', io.StringIO()):
            with self.assertRaisesRegex(ValueError, 'stale'):
                deploy.wait_for_release('https://example.pages.dev/', {})
            self.assertEqual(verify.call_count, 10)

    def test_invalid_production_endpoints(self):
        for endpoint in ['', 'ws://game.example.org/ws', 'https://game.example.org/ws',
                         'wss://user:pass@game.example.org/ws', 'wss://game.example.org/ws?token=a',
                         'wss://game.example.org/ws#fragment', 'wss://game.example.org/api',
                         'wss://localhost/ws', 'wss://a.localhost/ws', 'wss://127.0.0.2/ws',
                         'wss://192.168.1.2/ws', 'wss://[::1]/ws', 'wss://internal/ws',
                         'wss://game.example.org:0/ws', 'wss://game.example.org:65536/ws',
                         'wss://game.example.org:bad/ws', 'wss://game.example.org:/ws', 'wss://bad..example.org/ws',
                         'wss://-bad.example.org/ws']:
            with self.subTest(endpoint=endpoint), self.assertRaises(ValueError):
                deploy.websocket_url(endpoint)

    def test_public_endpoint_with_path_and_port(self):
        endpoint = 'wss://game.example.org:8443/block/ws'
        self.assertEqual(deploy.websocket_url(endpoint), endpoint)
        endpoint = 'wss://[2606:4700:4700::1111]/ws'
        self.assertEqual(deploy.websocket_url(endpoint), endpoint)

    def test_missing_credentials_fail_before_build(self):
        with patch.dict(os.environ, {'BS_WS_URL': 'wss://game.example.org/ws'}, clear=True), \
                patch('sys.argv', ['deploy_web.py', '--tag', 'web-test']), \
                patch.object(deploy, 'load_local_env'), \
                patch.object(deploy, 'run') as run:
            with self.assertRaisesRegex(ValueError, 'Missing deployment environment variables'):
                deploy.main()
            run.assert_not_called()

    def test_local_env_is_data_and_preserves_explicit_environment(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {'BS_WS_URL': 'wss://explicit.example.org/ws'}, clear=True):
            path = Path(directory) / '.env'
            path.write_text('export CLOUDFLARE_API_TOKEN="test-token" # local\n'
                            'CLOUDFLARE_ACCOUNT_ID=test-account\n'
                            'BS_WS_URL=wss://ignored.example.org/ws\n'
                            'PATH=/do-not-load\n')
            deploy.load_local_env(path)
            self.assertEqual(os.environ['CLOUDFLARE_API_TOKEN'], 'test-token')
            self.assertEqual(os.environ['CLOUDFLARE_ACCOUNT_ID'], 'test-account')
            self.assertEqual(os.environ['BS_WS_URL'], 'wss://explicit.example.org/ws')
            self.assertNotIn('PATH', os.environ)

    def test_local_env_error_does_not_echo_value(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / '.env'
            path.write_text('CLOUDFLARE_API_TOKEN=private token\n')
            with self.assertRaises(ValueError) as raised:
                deploy.load_local_env(path)
            self.assertNotIn('private', str(raised.exception))

    def test_csp_hashes_exact_inline_bootstrap(self):
        bootstrap = '\nimport init from "/client.js";\nawait init();\n'
        with tempfile.TemporaryDirectory() as directory:
            dist = Path(directory)
            (dist / 'index.html').write_text(f'<script type="module">{bootstrap}</script><script src="external.js"></script>')
            deploy.write_headers(dist, 'wss://game.example.org/ws')
            headers = (dist / '_headers').read_text()
        digest = base64.b64encode(hashlib.sha256(bootstrap.encode()).digest()).decode()
        self.assertIn(f"'sha256-{digest}'", headers)
        self.assertEqual(headers.count("'sha256-"), 1)
        self.assertIn("connect-src 'self' wss://game.example.org/ws", headers)
        self.assertIn("https://game.example.org/api/;", headers)  # /api/ 아래 프로필·리플레이·순위표
        self.assertNotIn("script-src 'self' 'unsafe-inline'", headers)

    def test_release_manifest_and_live_verification(self):
        html = b'<html>new release</html>'
        with tempfile.TemporaryDirectory() as directory:
            dist = Path(directory)
            (dist / 'index.html').write_bytes(html)
            release = deploy.write_release(dist, 'web-test')
            self.assertEqual(json.loads((dist / 'release.json').read_text()), release)
            self.assertEqual(release['index_sha256'], hashlib.sha256(html).hexdigest())
            self.assertNotEqual(deploy.write_release(dist, 'web-test')['deployment'], release['deployment'])
        def response(content, csp=True):
            stream = io.BytesIO(content)
            stream.status = 200
            stream.headers = {'Content-Security-Policy': "default-src 'self'"} if csp else {}
            return stream
        with patch.object(deploy, 'urlopen', side_effect=[response(json.dumps(release).encode()), response(html)]) as fetch:
            deploy.verify_release('https://example.pages.dev/', release)
            self.assertIn(release['deployment'], fetch.call_args_list[0].args[0].full_url)
        with patch.object(deploy, 'urlopen', return_value=response(json.dumps({**release, 'deployment': 'old'}).encode())):
            with self.assertRaisesRegex(ValueError, 'different release'):
                deploy.verify_release('https://example.pages.dev/', release)
        with patch.object(deploy, 'urlopen', side_effect=[response(json.dumps(release).encode()), response(b'old HTML')]):
            with self.assertRaisesRegex(ValueError, 'HTML does not match'):
                deploy.verify_release('https://example.pages.dev/', release)
        with patch.object(deploy, 'urlopen', return_value=response(html, csp=False)):
            with self.assertRaisesRegex(ValueError, 'CSP header check failed'):
                deploy.verify_release('https://example.pages.dev/', release)


if __name__ == '__main__':
    unittest.main()
