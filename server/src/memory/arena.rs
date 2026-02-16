// massive_game_server/server/src/memory/arena.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArenaHandle(pub usize);

#[derive(Debug)]
pub struct Arena<T> {
    slots: Vec<Option<T>>,
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
            self.slots[reused_idx] = Some(value);
            self.len += 1;
            return ArenaHandle(reused_idx);
        }

        let idx = self.slots.len();
        self.slots.push(Some(value));
        self.len += 1;
        ArenaHandle(idx)
    }

    pub fn get(&self, handle: ArenaHandle) -> Option<&T> {
        self.slots.get(handle.0).and_then(Option::as_ref)
    }

    pub fn get_mut(&mut self, handle: ArenaHandle) -> Option<&mut T> {
        self.slots.get_mut(handle.0).and_then(Option::as_mut)
    }

    pub fn dealloc(&mut self, handle: ArenaHandle) -> Option<T> {
        let slot = self.slots.get_mut(handle.0)?;
        let removed = slot.take();
        if removed.is_some() {
            self.free_list.push(handle.0);
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
        assert_eq!(c, a);
        assert_eq!(arena.get(b), Some(&2));
    }
}
