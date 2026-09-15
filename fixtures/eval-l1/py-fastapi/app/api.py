from fastapi import Depends, FastAPI

app = FastAPI()


class UserRepository:
    def find(self, user_id: str):
        return {"id": user_id}


class UserService:
    def __init__(self, repo: UserRepository):
        self.repo = repo

    def load(self, user_id: str):
        return self.repo.find(user_id)


def get_user_repository() -> UserRepository:
    return UserRepository()


def get_user_service(
    repo: UserRepository = Depends(get_user_repository),
) -> UserService:
    return UserService(repo)


@app.get("/users/{user_id}")
def read_user(user_id: str, svc: UserService = Depends(get_user_service)):
    return svc.load(user_id)


def load_plugin(name: str):
    import importlib

    return importlib.import_module(f"plugins.{name}")


def call_via_getattr(obj, method: str):
    return getattr(obj, "load")("x")
