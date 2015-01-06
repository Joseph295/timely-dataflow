use std::collections::{HashMap, HashSet};
use std::collections::hash_map::Entry::{Occupied, Vacant};
use std::hash::Hash;
use std::hash;
use core::fmt::Show;

use progress::count_map::CountMap;


use progress::{Timestamp, PathSummary, Scope};
use communication::channels::{Data};
use communication::exchange::{ExchangeReceiver, exchange_with};
use example::stream::Stream;
use communication::channels::OutputPort;

use progress::subgraph::Source::ScopeOutput;
use progress::subgraph::Target::ScopeInput;

pub trait DistinctExtensionTrait { fn distinct(&mut self) -> Self; }

impl<T: Timestamp, S: PathSummary<T>, D: Data+Hash+Eq+Show> DistinctExtensionTrait for Stream<T, S, D>
{
    fn distinct(&mut self) -> Stream<T, S, D>
    {
        let allocator = &mut (*self.allocator.borrow_mut());
        let (sender, receiver) = exchange_with(allocator, |record| hash::hash(&record));
        let scope = DistinctScope
        {
            index:      allocator.index(),
            input:      receiver,
            output:     OutputPort::new(),
            elements:   HashMap::new(),
            dispose:    Vec::new(),

            internal:   Vec::new(),
            external:   Vec::new(),
        };

        let targets = scope.output.targets.clone();
        let index = self.graph.add_scope(box scope);

        self.graph.connect(self.name, ScopeInput(index, 0));
        self.port.borrow_mut().push(box sender);

        return self.copy_with(ScopeOutput(index, 0), targets);
    }
}

pub struct DistinctScope<T: Timestamp, D: Data+Hash+Eq+PartialEq>
{
    index:      uint,
    input:      ExchangeReceiver<T, D>,
    output:     OutputPort<T, D>,
    elements:   HashMap<T, HashSet<D>>,
    dispose:    Vec<T>,

    internal:   Vec<(T, i64)>,
    external:   Vec<(T, i64)>,
}

impl<T: Timestamp+Hash+Eq, S: PathSummary<T>, D: Data+Hash+Eq+PartialEq+Show> Scope<T, S> for DistinctScope<T, D>
{
    fn inputs(&self) -> uint { 1 }
    fn outputs(&self) -> uint { 1 }

    fn push_external_progress(&mut self, external: &Vec<Vec<(T, i64)>>) -> ()
    {
        for &(time, val) in external[0].iter() { self.external.update(time, val); }
    }

    fn pull_internal_progress(&mut self, internal: &mut Vec<Vec<(T, i64)>>,
                                         consumed: &mut Vec<Vec<(T, i64)>>,
                                         produced: &mut Vec<Vec<(T, i64)>>) -> bool
    {
        // drain the input into sets.
        for (time, data) in self.input
        {
            let set = match self.elements.entry(&time)
            {
                Occupied(x) => x.into_mut(),
                Vacant(x)   =>
                {
                    self.internal.update(time, 1);
                    x.insert(HashSet::new())
                },
            };

            for &datum in data.iter() { set.insert(datum); }
        }

        // see if we should send any of it
        for key in self.elements.keys()
        {
            if self.external.iter().any(|&(t,_)| t.gt(key)) { self.dispose.push(*key); }
        }

        // send anything we have finalized
        while let Some(key) = self.dispose.pop()
        {
            let mut output = self.output.buffer_for(&key);
            if let Some(data) = self.elements.remove(&key)
            {
                for &datum in data.iter() { output.send(datum); }
            }
            self.internal.update(key, -1);
        }

        // extract what we know about progress from the input and output adapters.
        self.input.pull_progress(&mut consumed[0], &mut internal[0]);
        self.output.pull_progress(&mut produced[0]);

        while let Some((time, delta)) = self.internal.pop() { internal[0].update(time, delta); }

        return false;
    }

    fn name(&self) -> String { format!("Distinct") }
    fn notify_me(&self) -> bool { true }
}
