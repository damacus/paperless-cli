"""Stateful session management for the Paperless-ngx CLI.

Maintains ephemeral state across commands in a REPL session:
- last_query: most recent search query
- selected_docs: currently selected document IDs
- history: command history for the current session
"""

from __future__ import annotations

from paperless_ngx.utils.paperless_backend import load_session, save_session


class Session:
    """In-memory session state with optional disk persistence."""

    def __init__(self):
        self._state = load_session()

    @property
    def last_query(self) -> str:
        return self._state.get("last_query", "")

    @last_query.setter
    def last_query(self, value: str):
        self._state["last_query"] = value
        self._persist()

    @property
    def selected_docs(self) -> list[int]:
        return self._state.get("selected_docs", [])

    @selected_docs.setter
    def selected_docs(self, value: list[int]):
        self._state["selected_docs"] = value
        self._persist()

    @property
    def history(self) -> list[str]:
        return self._state.get("history", [])

    def add_history(self, command: str):
        """Append a command to session history (max 500 entries)."""
        h = self._state.setdefault("history", [])
        h.append(command)
        if len(h) > 500:
            self._state["history"] = h[-500:]
        self._persist()

    def clear_selection(self):
        """Clear selected document IDs."""
        self._state["selected_docs"] = []
        self._persist()

    def to_dict(self) -> dict:
        return dict(self._state)

    def _persist(self):
        save_session(self._state)


# Module-level singleton used by the REPL
_session: Session | None = None


def get_session() -> Session:
    """Get or create the module-level session singleton."""
    global _session
    if _session is None:
        _session = Session()
    return _session
