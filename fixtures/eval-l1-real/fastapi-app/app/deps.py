from .repository import InMemoryUserRepository
from .service import UserService


def get_user_repository() -> InMemoryUserRepository:
    return InMemoryUserRepository()


def get_user_service() -> UserService:
    return UserService(get_user_repository())
