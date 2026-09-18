// Real-idiom multi-file Go interface assert + method-set (M3-B).

package store

type Repository interface {
	Get(id string) string
	Put(id string, v string)
}

type Cache interface {
	Get(id string) string
}

type MemRepo struct {
	data map[string]string
}

func (m *MemRepo) Get(id string) string {
	if m.data == nil {
		return ""
	}
	return m.data[id]
}

func (m *MemRepo) Put(id string, v string) {
	if m.data == nil {
		m.data = map[string]string{}
	}
	m.data[id] = v
}

var _ Repository = (*MemRepo)(nil)

// RedisCache implements Cache by method-set name match only.
type RedisCache struct{}

func (r *RedisCache) Get(id string) string {
	return "redis:" + id
}
