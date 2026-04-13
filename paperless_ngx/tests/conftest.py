"""pytest configuration and fixtures for cli-anything-paperless-ngx tests.

Ensures that real config and session files on the developer's machine are
never read or written during the test run.
"""

from __future__ import annotations

import pytest


@pytest.fixture(autouse=True)
def isolate_config(tmp_path, monkeypatch):
    """Redirect config and session paths to tmp_path for every test.

    This prevents tests from reading/writing the real
    ~/.config/paperless-cli/config.json or /tmp/paperless-cli-session.json.
    """
    fake_home = tmp_path / "home"
    fake_config = str(fake_home / ".config/paperless-cli/config.json")
    fake_session = str(fake_home / "paperless-cli-session.json")

    fake_home.mkdir(parents=True, exist_ok=True)
    monkeypatch.setenv("HOME", str(fake_home))

    import paperless_ngx.utils.paperless_backend as be_mod

    monkeypatch.setattr(be_mod, "CONFIG_PATH", fake_config)
    monkeypatch.setattr(be_mod, "SESSION_PATH", fake_session)

    # Also patch the session module's imports so they pick up the redirected paths
    import paperless_ngx.core.session as sess_mod

    # Reset singleton so each test starts fresh
    monkeypatch.setattr(sess_mod, "_session", None)
