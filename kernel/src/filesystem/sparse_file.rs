use alloc::{boxed::Box, collections::BTreeMap};

const PAGE_SIZE: usize = 4096;

#[derive(Default)]
pub struct SparseFileData {
    len: usize,
    pages: BTreeMap<usize, Box<[u8; PAGE_SIZE]>>,
}

impl SparseFileData {
    pub const fn new() -> Self {
        Self {
            len: 0,
            pages: BTreeMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn ensure_len(&mut self, len: usize) {
        self.len = self.len.max(len);
    }

    pub fn zero_range(&mut self, offset: usize, len: usize) {
        let end = offset.saturating_add(len).min(self.len);
        if offset >= end {
            return;
        }

        let first_page = offset / PAGE_SIZE;
        let last_page = (end - 1) / PAGE_SIZE;
        for page_index in first_page..=last_page {
            let page_start = page_index * PAGE_SIZE;
            let range_start = offset.saturating_sub(page_start);
            let range_end = (end - page_start).min(PAGE_SIZE);
            if let Some(page) = self.pages.get_mut(&page_index) {
                page[range_start..range_end].fill(0);
            }
        }
    }

    pub fn truncate(&mut self, len: usize) {
        if len >= self.len {
            self.len = len;
            return;
        }

        let keep_pages = len.div_ceil(PAGE_SIZE);
        self.pages.retain(|page_index, _| *page_index < keep_pages);
        if !len.is_multiple_of(PAGE_SIZE)
            && let Some(page) = self.pages.get_mut(&(len / PAGE_SIZE))
        {
            page[len % PAGE_SIZE..].fill(0);
        }
        self.len = len;
    }

    pub fn read_at(&self, buffer: &mut [u8], offset: usize) -> usize {
        if offset >= self.len || buffer.is_empty() {
            return 0;
        }

        let total = buffer.len().min(self.len - offset);
        buffer[..total].fill(0);
        let mut copied = 0usize;

        while copied < total {
            let absolute = offset + copied;
            let page_index = absolute / PAGE_SIZE;
            let page_offset = absolute % PAGE_SIZE;
            let chunk_len = (total - copied).min(PAGE_SIZE - page_offset);

            if let Some(page) = self.pages.get(&page_index) {
                buffer[copied..copied + chunk_len]
                    .copy_from_slice(&page[page_offset..page_offset + chunk_len]);
            }

            copied += chunk_len;
        }

        total
    }

    pub fn write_at(&mut self, offset: usize, buffer: &[u8]) -> usize {
        if buffer.is_empty() {
            return 0;
        }

        let end = offset.saturating_add(buffer.len());
        self.len = self.len.max(end);
        let mut written = 0usize;

        while written < buffer.len() {
            let absolute = offset + written;
            let page_index = absolute / PAGE_SIZE;
            let page_offset = absolute % PAGE_SIZE;
            let chunk_len = (buffer.len() - written).min(PAGE_SIZE - page_offset);
            let page = self
                .pages
                .entry(page_index)
                .or_insert_with(|| Box::new([0; PAGE_SIZE]));
            page[page_offset..page_offset + chunk_len]
                .copy_from_slice(&buffer[written..written + chunk_len]);
            written += chunk_len;
        }

        buffer.len()
    }
}
