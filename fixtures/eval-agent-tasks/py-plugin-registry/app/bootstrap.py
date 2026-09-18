from .registry import PluginRegistry
from .plugins import MetricsPlugin


def bootstrap():
    reg = PluginRegistry()
    reg.register("metrics", MetricsPlugin)
    return reg.create("metrics")
