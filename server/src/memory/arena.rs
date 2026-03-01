// massive_game_server/server/src/memory/arena.rs

/// Handle to an arena-allocated slot. Includes a generation counter so that
/// stale handles (pointing at a slot that was deallocated and then reused)
/// are detected rather than silently returning the wrong value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArenaHandle {
    pub index: usize,
    pub generation: u64,
}

#[derive(Debug)]
struct Slot<T> {
    value: Option<T>,
    generation: u64,
}

#[derive(Debug)]
pub struct Arena<T> {
    slots: Vec<Slot<T>>,
    free_list: Vec<usize>,
    len: usize,
}

impl<T> Arena<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            slots: Vec::with_capacity(capacity),
            free_list: Vec::new(),
            len: 0,
        }
    }

    pub fn alloc(&mut self, value: T) -> ArenaHandle {
        if let Some(reused_idx) = self.free_list.pop() {
            let slot = &mut self.slots[reused_idx];
            slot.value = Some(value);
            // Generation was already bumped on dealloc; the new handle
            // carries the current (bumped) generation so old handles are stale.
            self.len += 1;
            return ArenaHandle {
                index: reused_idx,
                generation: slot.generation,
            };
        }

        let idx = self.slots.len();
        self.slots.push(Slot {
            value: Some(value),
            generation: 0,
        });
        self.len += 1;
        ArenaHandle {
            index: idx,
            generation: 0,
        }
    }

    pub fn get(&self, handle: ArenaHandle) -> Option<&T> {
        let slot = self.slots.get(handle.index)?;
        if slot.generation != handle.generation {
            return None; // Stale handle — slot has been recycled.
        }
        slot.value.as_ref()
    }

    pub fn get_mut(&mut self, handle: ArenaHandle) -> Option<&mut T> {
        let slot = self.slots.get_mut(handle.index)?;
        if slot.generation != handle.generation {
            return None; // Stale handle — slot has been recycled.
        }
        slot.value.as_mut()
    }

    pub fn dealloc(&mut self, handle: ArenaHandle) -> Option<T> {
        let slot = self.slots.get_mut(handle.index)?;
        if slot.generation != handle.generation {
            return None; // Stale handle — already deallocated and potentially reused.
        }
        let removed = slot.value.take();
        if removed.is_some() {
            // Bump generation so that any outstanding handles to this slot
            // become invalid. Wrapping is fine — it takes 2^32 alloc/dealloc
            // cycles on the same slot to create an ABA collision.
            slot.generation = slot.generation.wrapping_add(1);
            self.free_list.push(handle.index);
            self.len = self.len.saturating_sub(1);
        }
        removed
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arena_reuses_slots() {
        let mut arena = Arena::with_capacity(2);
        let a = arena.alloc(1);
        let b = arena.alloc(2);
        assert_eq!(arena.dealloc(a), Some(1));
        let c = arena.alloc(3);
        // Same index reused, but different generation.
        assert_eq!(c.index, a.index);
        assert_ne!(c.generation, a.generation);
        assert_eq!(arena.get(c), Some(&3));
        assert_eq!(arena.get(b), Some(&2));
    }

    #[test]
    fn stale_handle_returns_none() {
        let mut arena = Arena::with_capacity(4);
        let h1 = arena.alloc(10);
        assert_eq!(arena.get(h1), Some(&10));

        // Deallocate, then allocate a new value in the same slot.
        arena.dealloc(h1);
        let h2 = arena.alloc(20);
        assert_eq!(h2.index, h1.index);

        // The old handle must NOT see the new value.
        assert_eq!(arena.get(h1), None);
        assert_eq!(arena.get_mut(h1), None);

        // The new handle sees the new value.
        assert_eq!(arena.get(h2), Some(&20));
    }

    #[test]
    fn double_dealloc_is_safe() {
        let mut arena = Arena::with_capacity(4);
        let h = arena.alloc(42);
        assert_eq!(arena.dealloc(h), Some(42));
        // Second dealloc with the same (now-stale) handle returns None.
        assert_eq!(arena.dealloc(h), None);
        assert_eq!(arena.len(), 0);
    }

    #[test]
    fn stale_handle_after_reuse_cycle() {
        let mut arena = Arena::with_capacity(2);
        let original = arena.alloc("first");

        // Deallocate and reallocate multiple times in the same slot.
        arena.dealloc(original);
        let second = arena.alloc("second");
        arena.dealloc(second);
        let third = arena.alloc("third");

        // All prior handles are stale.
        assert_eq!(arena.get(original), None);
        assert_eq!(arena.get(second), None);
        assert_eq!(arena.get(third), Some(&"third"));
    }

    #[test]
    fn generation_increments_per_dealloc() {
        let mut arena = Arena::with_capacity(1);
        let h0 = arena.alloc(0);
        assert_eq!(h0.generation, 0);

        arena.dealloc(h0);
        let h1 = arena.alloc(1);
        assert_eq!(h1.generation, 1);

        arena.dealloc(h1);
        let h2 = arena.alloc(2);
        assert_eq!(h2.generation, 2);
    }
}
