// Public synthetic golden — Go interface assert + method-set (M3-B / Track M5).
// Candidates only — not sound.

package iface

type Store interface {
	Get(id string) string
	Put(id string, v string)
}

type MemStore struct{}

func (m *MemStore) Get(id string) string { return id }

func (m *MemStore) Put(id string, v string) {}

// Compile-time assertion: go.di.interface_impl_v2 Heuristic edges.
var _ Store = (*MemStore)(nil)

func Use(s Store) string {
	return s.Get("k")
}
