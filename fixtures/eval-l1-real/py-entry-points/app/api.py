# API consumer using Depends/Security + plugins.

from typing import Annotated

from fastapi import Depends, Security

from .plugins import get_current_user, get_user_service, load_plugins


def read_user(
    svc: Annotated[object, Depends(get_user_service)],
    user=Security(get_current_user),
):
    plugins = load_plugins()
    return {"user": user, "plugins": len(list(plugins)), "svc": svc}
