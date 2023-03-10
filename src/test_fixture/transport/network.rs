use crate::{
    helpers::{transport::Transport, HelperIdentity},
    sync::{Arc, Weak},
};

use crate::test_fixture::transport::InMemoryTransport;

/// Container for all active transports
#[derive(Clone)]
pub struct InMemoryNetwork {
    pub transports: [Arc<InMemoryTransport>; 3],
}

impl Default for InMemoryNetwork {
    fn default() -> Self {
        let [mut first, mut second, mut third] = [
            InMemoryTransport::with_stub_callbacks(1.try_into().unwrap()),
            InMemoryTransport::with_stub_callbacks(2.try_into().unwrap()),
            InMemoryTransport::with_stub_callbacks(3.try_into().unwrap()),
        ];
        first.connect(&mut second);
        second.connect(&mut third);
        third.connect(&mut first);

        Self {
            transports: [first.start(), second.start(), third.start()],
        }
    }
}

impl InMemoryNetwork {
    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn helper_identities(&self) -> [HelperIdentity; 3] {
        self.transports
            .iter()
            .map(|t| t.identity())
            .collect::<Vec<_>>()
            .try_into()
            .unwrap()
    }

    #[must_use]
    pub fn transport(&self, id: HelperIdentity) -> Option<impl Transport> {
        self.transports
            .iter()
            .find(|t| t.identity() == id)
            .map(Arc::downgrade)
    }

    #[allow(clippy::missing_panics_doc)]
    #[must_use]
    pub fn transports(&self) -> [impl Transport + Clone; 3] {
        let transports: [Weak<InMemoryTransport>; 3] = self
            .transports
            .iter()
            .map(Arc::downgrade)
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| "What is dead may never die")
            .unwrap();
        transports
    }
}
