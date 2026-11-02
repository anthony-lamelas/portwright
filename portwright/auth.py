"""Gives agent sessions the least login access that works.

Subscription mode copies only a short-lived access token into a session. The refresh
token never enters a sandbox, so a container can't outlive its token or affect the
host's login. API-key mode passes the key the user set in their environment.
"""

from __future__ import annotations

import json
import os
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path

CREDENTIALS = Path.home() / ".claude" / ".credentials.json"
# A session shouldn't start with a token that could expire partway through.
MINIMUM_TOKEN_LIFE_SECONDS = 2 * 60 * 60
SUBSCRIPTION = "subscription"
API_KEY = "api_key"


class AuthError(RuntimeError):
    """Login isn't usable, and retrying won't help."""


@dataclass
class Auth:
    mode: str = SUBSCRIPTION
    credentials_path: Path = CREDENTIALS

    def check_environment(self) -> None:
        """Refuse a setup that would bill or authenticate differently than asked."""
        key_set = bool(os.environ.get("ANTHROPIC_API_KEY"))
        if self.mode == SUBSCRIPTION and key_set:
            raise AuthError(
                "ANTHROPIC_API_KEY is set, so Claude Code would bill that key instead of "
                "your subscription. Unset it, or run with --auth api_key."
            )
        if self.mode == API_KEY and not key_set:
            raise AuthError("--auth api_key needs ANTHROPIC_API_KEY to be set.")
        if self.mode not in (SUBSCRIPTION, API_KEY):
            raise AuthError(f"unknown auth mode {self.mode}")

    def _read_oauth(self) -> dict:
        try:
            return json.loads(self.credentials_path.read_text())["claudeAiOauth"]
        except (OSError, KeyError, ValueError) as error:
            raise AuthError(f"no Claude login found at {self.credentials_path}: run `claude` once") from error

    def _refresh_on_host(self) -> None:
        """Have the host's own Claude Code renew the token, using a tiny request."""
        subprocess.run(
            ["claude", "-p", "Reply with OK.", "--max-turns", "1", "--output-format", "json"],
            capture_output=True, text=True, timeout=180, check=False, cwd=Path.home(),
        )

    def prepare_session(self, home: str | Path) -> dict[str, str]:
        """Set up a session's home folder and return environment variables for it."""
        self.check_environment()
        if self.mode == API_KEY:
            return {"ANTHROPIC_API_KEY": os.environ["ANTHROPIC_API_KEY"]}
        oauth = self._read_oauth()
        if oauth.get("expiresAt", 0) / 1000 < time.time() + MINIMUM_TOKEN_LIFE_SECONDS:
            self._refresh_on_host()
            oauth = self._read_oauth()
        limited = {
            key: oauth[key]
            for key in ("accessToken", "expiresAt", "scopes", "subscriptionType", "rateLimitTier")
            if key in oauth
        }
        directory = Path(home) / ".claude"
        directory.mkdir(parents=True, exist_ok=True)
        path = directory / ".credentials.json"
        path.touch(mode=0o600, exist_ok=True)
        path.write_text(json.dumps({"claudeAiOauth": limited}))
        return {}
