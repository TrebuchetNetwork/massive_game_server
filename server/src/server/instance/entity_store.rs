use parking_lot::{
    RwLock as ParkingLotRwLock, RwLockReadGuard as ParkingLotRwLockReadGuard,
    RwLockWriteGuard as ParkingLotRwLockWriteGuard,
};

pub struct AuthoritativeEntityStore<T> {
    entities: ParkingLotRwLock<Vec<T>>,
}

impl<T> AuthoritativeEntityStore<T> {
    pub fn new(initial: Vec<T>) -> Self {
        Self {
            entities: ParkingLotRwLock::new(initial),
        }
    }

    #[inline]
    pub fn read(&self) -> ParkingLotRwLockReadGuard<'_, Vec<T>> {
        self.entities.read()
    }

    #[inline]
    pub fn write(&self) -> ParkingLotRwLockWriteGuard<'_, Vec<T>> {
        self.entities.write()
    }

    #[inline]
    pub fn extend<I>(&self, iter: I)
    where
        I: IntoIterator<Item = T>,
    {
        self.entities.write().extend(iter);
    }

    #[inline]
    pub fn take_all(&self) -> Vec<T> {
        let mut guard = self.entities.write();
        std::mem::take(&mut *guard)
    }

    #[inline]
    pub fn replace_all(&self, next: Vec<T>) {
        *self.entities.write() = next;
    }
}

#[cfg(test)]
mod tests {
    use super::AuthoritativeEntityStore;

    #[test]
    fn entity_store_take_extend_and_replace_round_trip() {
        let store = AuthoritativeEntityStore::new(vec![1, 2, 3]);
        assert_eq!(store.read().len(), 3);

        let drained = store.take_all();
        assert_eq!(drained, vec![1, 2, 3]);
        assert_eq!(store.read().len(), 0);

        store.extend([4, 5]);
        assert_eq!(store.read().as_slice(), &[4, 5]);

        store.replace_all(vec![9]);
        assert_eq!(store.read().as_slice(), &[9]);
    }
}
