// M3-B: Go interface implementation systematized.
// `var _ I = (*T)(nil)` + method-set name match for indexed types.
// Heuristic `go.di.interface_impl_v2`. Candidates only — not sound.

package store

type Store interface {
	Get(id string) string
	Put(id string, v string)
}

type Cache interface {
	Get(id string) string
}

type MemStore struct {
	data map[string]string
}

func (m *MemStore) Get(id string) string {
	return m.data[id]
}

func (m *MemStore) Put(id string, v string) {
	if m.data == nil {
		m.data = map[string]string{}
	}
	m.data[id] = v
}

// Compile-time assertion: MemStore implements Store.
var _ Store = (*MemStore)(nil)

// RedisCache implements Cache by method-set name match (no assertion).
type RedisCache struct{}

func (r *RedisCache) Get(id string) string {
	return id
}

func UseStore(s Store) string {
	return s.Get("k")
}
