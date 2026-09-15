class UserService:
    def __init__(self, repo):
        self.repo = repo

    def load(self, user_id: str):
        return self.repo.find(user_id)
