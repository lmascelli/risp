use crate::value::object::Object;

const SLOTS_PER_PAGE: usize = 256;

struct Slot {
    generation: u64,
    object: Object,
}

impl Slot {
    pub(crate) fn new(object: Object) -> Self {
        Self {
            generation: 1,
            object,
        }
    }
}

struct Page {
    slots: Vec<Slot>,
}

impl Page {
    pub(crate) fn new() -> Self {
        Self {
            slots: Vec::with_capacity(SLOTS_PER_PAGE),
        }
    }

    pub(crate) fn push(&mut self, object: Object) {
        self.slots.push(Slot::new(object))
    }

    pub(crate) fn is_full(&self) -> bool {
        self.slots.len() == SLOTS_PER_PAGE
    }

    pub(crate) fn last_free_index(&self) -> usize {
        self.slots.len()
    }
}

struct Arena {
    pages: Vec<Page>,
}

impl Arena {
    pub(crate) fn new() -> Self {
        Self {
            pages: Vec::new(),
        }
    }

    fn last_page(&self) -> &Page {
        &self.pages[self.pages.len() - 1]
    }

    fn last_page_mut(&mut self) -> &mut Page {
        let last_page_index = self.pages.len() - 1;
        &mut self.pages[last_page_index]
    }

    pub(crate) fn allocate(&mut self, object: Object) -> (usize, usize) {
        if self.last_page().is_full() {
            self.pages.push(Page::new());
        }

        let page = self.last_page_mut();
        let index = page.last_free_index();
        page.push(object);
        (index, self.pages.len())
    }
}