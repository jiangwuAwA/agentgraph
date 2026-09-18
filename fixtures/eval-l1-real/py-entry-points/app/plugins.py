# Real-idiom multi-file Python plugins + entry points (M3-C).

from importlib.metadata import entry_points
from typing import Annotated

from fastapi import Depends, Security


class Plugin:
    def __init_subclass__(cls, **kwargs):
        super().__init_subclass__(**kwargs)


class AuditPlugin(Plugin):
    def run(self) -> int:
        return 0


def get_user_service():
    return object()


def get_current_user():
    return {"sub": "demo"}


def load_plugins():
    return list(entry_points(group="myapp.audit"))
