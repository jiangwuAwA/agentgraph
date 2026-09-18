class PluginRegistry:
    def __init__(self):
        self._plugins = {}

    def register(self, name: str, factory):
        self._plugins[name] = factory
        return name

    def create(self, name: str):
        factory = self._plugins.get(name)
        return factory() if factory else None
