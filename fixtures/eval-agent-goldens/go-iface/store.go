package store

// Public synthetic Go interface golden (P1-3 implementor edge_role).

type Store interface {
	Get(key string) (string, bool)
	Put(key string, val string)
}

type MemStore struct {
	m map[string]string
}

func NewMemStore() *MemStore {
	return &MemStore{m: map[string]string{}}
}

func (s *MemStore) Get(key string) (string, bool) {
	v, ok := s.m[key]
	return v, ok
}

func (s *MemStore) Put(key string, val string) {
	s.m[key] = val
}

var _ Store = (*MemStore)(nil)

func UseStore(s Store) string {
	v, _ := s.Get("k")
	s.Put("k", v)
	return v
}
