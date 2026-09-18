# M3-C: Python entry-point-like gaps + FastAPI Security / Annotated Depends.
# Heuristic `py.di.entry_points` / existing `py.di.depends`. Not sound.

from importlib.metadata import entry_points
from typing import Annotated

from fastapi import Depends, Security


class Plugin:
    registry = []

    def __init_subclass__(cls, **kwargs):
        super().__init_subclass__(**kwargs)
        Plugin.registry.append(cls)


class AuthPlugin(Plugin):
    def run(self):
        return 1


def get_current_user():
    return {"sub": "demo"}


def get_user_service():
    return object()


def read_user(
    svc: Annotated[object, Depends(get_user_service)],
    user=Security(get_current_user),
):
    return user


def load_plugins():
    eps = entry_points(group="myapp.plugins")
    return [ep.load() for ep in eps]


def load_pkg_resources_plugins():
    import pkg_resources

    return list(pkg_resources.iter_entry_points("myapp.plugins"))
