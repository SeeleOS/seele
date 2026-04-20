use core::{
    alloc::{GlobalAlloc, Layout},
    ptr::{self, NonNull},
};

use crate::memory::utils::Locked;
use core::mem;

const BLOCK_SIZES: &[usize] = &[8, 16, 32, 64, 128, 256, 512, 1024, 2048];

struct Node {
    next: Option<&'static mut Node>,
}

pub struct FixedBlockSizeAllocator {
    heads: [Option<&'static mut Node>; BLOCK_SIZES.len()],
    fallback: linked_list_allocator::Heap,
}

impl FixedBlockSizeAllocator {
    pub const fn new() -> Self {
        Self {
            heads: [const { None }; BLOCK_SIZES.len()],
            fallback: linked_list_allocator::Heap::empty(),
        }
    }

    /// # Safety
    ///
    /// The caller must provide a valid, unused heap range that is exclusively
    /// owned by this allocator for the duration of its lifetime.
    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        unsafe {
            self.fallback.init(heap_start, heap_size);
        }
    }

    fn fallback_alloc(&mut self, layout: Layout) -> *mut u8 {
        match self.fallback.allocate_first_fit(layout) {
            Ok(ptr) => ptr.as_ptr(),
            Err(_) => ptr::null_mut(),
        }
    }
}

impl Default for FixedBlockSizeAllocator {
    fn default() -> Self {
        Self::new()
    }
}

fn get_correct_size_index(layout: &Layout) -> Option<usize> {
    // check which is higher, the size of it, or the aligh requirement
    let required_size = layout.size().max(layout.align());
    // check the first block that have a size more then the required size.
    BLOCK_SIZES.iter().position(|&s| s >= required_size)
}

unsafe impl GlobalAlloc for Locked<FixedBlockSizeAllocator> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut self_m = self.lock();

        match get_correct_size_index(&layout) {
            Some(index) => {
                // gets the head of the correct size block type and take it
                match self_m.heads[index].take() {
                    // returns the current head and updates the head to the next
                    // avalible node
                    Some(node) => {
                        self_m.heads[index] = node.next.take();
                        node as *mut Node as *mut u8
                    }
                    // allocates a new block if there is no avalible block
                    None => {
                        let block_size = BLOCK_SIZES[index];
                        let layout = Layout::from_size_align(block_size, block_size).unwrap();
                        self_m.fallback_alloc(layout)
                    }
                }
            }
            // falls back to the fallback alloc if its higher then 2kib (cant find a correct size)
            None => self_m.fallback_alloc(layout),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let mut self_m = self.lock();

        match get_correct_size_index(&layout) {
            // overwrites the de-allocated memory with a node pointing to the original head
            // aka overwritting the deallocated mem into the node and then plugging it to the head
            Some(index) => {
                // Points to the original head
                let new_node = Node {
                    next: self_m.heads[index].take(),
                };

                // doing some checks that i dont actually know what is this
                // but its probably important
                assert!(mem::size_of::<Node>() <= BLOCK_SIZES[index]);
                assert!(mem::align_of::<Node>() <= BLOCK_SIZES[index]);

                // the pointer to the new Node
                // aka the deallocated memory
                let new_node_ptr = ptr as *mut Node;
                unsafe {
                    // writes the new_node to the deallocated memory
                    // so the deallocated memory is now the new node
                    new_node_ptr.write(new_node);
                    // plug the new node to the head (the new head now points to the original head)
                    self_m.heads[index] = Some(&mut *new_node_ptr);
                }
            }

            // deallocate with the fallback alloc
            None => {
                let ptr = NonNull::new(ptr).unwrap();
                unsafe { self_m.fallback.deallocate(ptr, layout) };
            }
        }
    }
}
