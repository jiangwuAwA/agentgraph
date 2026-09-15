from fastapi import APIRouter, Depends, HTTPException

from .deps import get_user_service
from .schemas import UserOut
from .service import UserService

router = APIRouter()


@router.get("/users/{user_id}", response_model=UserOut)
def read_user(user_id: str, svc: UserService = Depends(get_user_service)):
    user = svc.load(user_id)
    if user is None:
        raise HTTPException(status_code=404, detail="not found")
    return user
