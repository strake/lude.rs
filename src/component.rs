use containers::collections::*;
use core::any::TypeId;
use core::{ptr, slice};
use core::ops::*;
use core::ptr::Unique;
use loca::*;

type Mask = usize;

pub struct ECS<A: Alloc> {
    alloc: A,
    masks: RawVec<Mask, A>,
    component_ptrs: HashTable<TypeId, *mut u8, ::sip::SipHasher, A>,
    droppers: HashTable<TypeId, fn(*mut u8, usize, &mut A), ::sip::SipHasher, A>
}

impl<A: Alloc + Clone> ECS<A> {
    #[inline]
    pub fn with_capacity_in(a: A, cap: usize) -> Option<Self> {
        let mut masks = RawVec::with_capacity_in(a.clone(), cap)?;
        for k in 0..cap { unsafe { masks.storage_mut()[k] = 0; } }
        let cps = HashTable::new_in(a.clone(), (0: Mask).trailing_zeros(), Default::default())?;
        let ds  = HashTable::new_in(a.clone(), (0: Mask).trailing_zeros(), Default::default())?;
        Some(ECS { alloc: a, masks: masks, component_ptrs: cps, droppers: ds })
    }
}

impl<A: Alloc> ECS<A> {
    #[inline]
    pub fn reg<C: 'static>(&mut self) -> Option<()> {
        let alloc = &mut self.alloc;
        let cap = self.masks.capacity();
        self.droppers.insert(TypeId::of::<C>(), drop_components::<C, A>).ok()?;
        let ptr: Unique<C> = alloc.alloc_array(cap).ok()?;
        match self.component_ptrs.insert_with(TypeId::of::<C>(), |p_opt| match p_opt {
            None => ptr.as_ptr() as _,
            Some(p) => unsafe { let _ = alloc.dealloc_array(ptr, cap); p },
        }) {
            Ok(_) => Some(()),
            Err((_, ptr)) => unsafe {
                Unique::new(ptr).map(|ptr| alloc.dealloc_array(ptr, cap));
                None
            }
        }
    }

    #[inline]
    fn component<C: 'static>(&self) -> Option<(usize, *mut C)> {
        self.component_ptrs.find_with_ix(&TypeId::of::<C>()).map(|(k, _, &ptr)| (k, ptr as _))
    }

    #[inline]
    pub fn get<C: 'static>(&self, k: usize) -> Option<&C> {
        let (ck, ptr) = self.component()?;
        unsafe { if 0 != self.masks.storage()[k] & 1 << ck { ptr.offset(k as _).as_ref() }
                 else { None } }
    }

    #[inline]
    pub fn modify<C: 'static, F: FnOnce(Option<C>) -> Option<C>>(&mut self, k: usize, f: F) { unsafe {
        let (ck, ptr) = match self.component() { None => return, Some(x) => x };
        let ptr = ptr.offset(k as _);
        match f(if 0 == self.masks.storage()[k] & 1 << ck { None } else {
            self.masks.storage_mut()[k] &= !(1 << ck);
            Some(ptr::read(ptr))
        }) {
            None => (),
            Some(x) => {
                self.masks.storage_mut()[k] |= 1 << ck;
                ptr::write(ptr, x);
            },
        }
    } }
}

impl<A: Alloc> Drop for ECS<A> {
    fn drop(&mut self) {
        for ((_, &ptr), (_, &drop)) in Iterator::zip(self.component_ptrs.iter(),
                                                     self.droppers.iter()) {
            drop(ptr, self.masks.capacity(), &mut self.alloc)
        }
    }
}

fn drop_components<C, A: Alloc>(ptr: *mut u8, n: usize, a: &mut A) { unsafe {
    for c in slice::from_raw_parts_mut(ptr as *mut C, n) { ptr::drop_in_place(c); }
    a.dealloc(ptr, Layout::array::<C>(n).unwrap());
} }
