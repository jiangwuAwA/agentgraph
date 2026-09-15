class InMemoryUserRepository:
    def __init__(self):
        self._db = {}

    def find(self, user_id: str):
        return self._db.get(user_id)

    def save(self, user_id: str, row):
        self._db[user_id] = row
