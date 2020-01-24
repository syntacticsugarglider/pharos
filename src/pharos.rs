use crate::{
    events::Sender, import::*, Channel, Error, ErrorKind, Events, Observable, ObserveConfig,
};

/// The Pharos lighthouse. When you implement [Observable] on your type, you can forward
/// the [`observe`](Observable::observe) method to Pharos and use [SinkExt::send](https://docs.rs/futures-preview/0.3.0-alpha.19/futures/sink/trait.SinkExt.html#method.send) to notify observers.
///
/// You can of course create several `Pharos` (I know, historical sacrilege) for (different) types
/// of events.
///
/// Please see the docs for [Observable] for an example. Others can be found in the README and
/// the [examples](https://github.com/najamelan/pharos/tree/master/examples) directory of the repository.
///
/// ## Implementation.
///
/// Currently just holds a `Vec<Option<Sender>>`. It will drop observers if the channel has
/// returned an error, which means it is closed or disconnected. However, we currently don't
/// compact the vector. Slots are reused for new observers, but the vector never shrinks.
///
/// **Note**: we only detect that observers can be removed when [SinkExt::send](https://docs.rs/futures-preview/0.3.0-alpha.19/futures/sink/trait.SinkExt.html#method.send) or [Pharos::num_observers]
/// is being called. Otherwise, we won't find out about disconnected observers and the vector of observers
/// will not mark deleted observers and thus their slots can not be reused.
///
/// The [Sink](https://docs.rs/futures-preview/0.3.0-alpha.19/futures/sink/trait.Sink.html) impl
/// is not very optimized for the moment. It just loops over all observers in each poll method
/// so it will call `poll_ready` and `poll_flush` again for observers that already returned `Poll::Ready(Ok(()))`.
///
/// TODO: I will do some benchmarking and see if this can be improved, eg. by keeping a state which tracks which
/// observers we still have to poll.
//
pub struct Pharos<Event>
where
    Event: 'static + Clone + Sync + Send,
{
    // Observers never get moved. Their index stays stable, so that when we free a slot,
    // we can store that in `free_slots`.
    //
    observers: Vec<Option<Sender<Event>>>,
    free_slots: Vec<usize>,
    state: State,
}

#[derive(Clone, Debug, PartialEq)]
//
enum State {
    Ready,
    Closed,
}

impl<Event> fmt::Debug for Pharos<Event>
where
    Event: 'static + Clone + Sync + Send,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pharos::Pharos<{}>", type_name::<Event>())
    }
}

impl<Event> Pharos<Event>
where
    Event: 'static + Clone + Sync + Send,
{
    /// Create a new Pharos. May it's light guide you to safe harbor.
    ///
    /// You can set the initial capacity of the vector of observers, if you know you will a lot of observers
    /// it will save allocations by setting this to a higher number.
    ///
    /// For pharos 0.4.0 on x64 Linux: `std::mem::size_of::<Option<Sender<_>>>() == 56 bytes`.
    //
    pub fn new(capacity: usize) -> Self {
        Self {
            observers: Vec::with_capacity(capacity),
            free_slots: Vec::with_capacity(capacity),
            state: State::Ready,
        }
    }

    /// Returns the size of the vector used to store the observers. Useful for debugging and testing if it
    /// seems to get to big.
    //
    pub fn storage_len(&self) -> usize {
        self.observers.len()
    }

    /// Returns the number of actual observers that are still listening (have not closed or dropped the [Events]).
    /// This will loop and it will verify for each if they are closed, clearing them from the internal storage
    /// if they are closed. This is similar to what notify does, but without sending an event.
    //
    pub fn num_observers(&mut self) -> usize {
        let mut count = 0;

        for (i, opt) in self.observers.iter_mut().enumerate() {
            if let Some(observer) = opt {
                if !observer.is_closed() {
                    count += 1;
                } else {
                    self.free_slots.push(i);
                    *opt = None
                }
            }
        }

        count
    }
}

/// Creates a new pharos, using 10 as the initial capacity of the vector used to store
/// observers. If this number does really not fit your use case, call [Pharos::new].
//
impl<Event> Default for Pharos<Event>
where
    Event: 'static + Clone + Sync + Send,
{
    fn default() -> Self {
        Self::new(10)
    }
}

impl<Event> Observable<Event> for Pharos<Event>
where
    Event: 'static + Clone + Sync + Send,
{
    type Error = Error;

    /// Will re-use slots from disconnected observers to avoid growing to much.
    ///
    /// TODO: provide API for the client to compact the pharos object after reducing the
    ///       number of observers.
    //
    fn observe(&mut self, options: ObserveConfig<Event>) -> Result<Events<Event>, Self::Error> {
        if self.state == State::Closed {
            return Err(ErrorKind::Closed.into());
        }

        match options.channel {
            Channel::Bounded(queue_size) => {
                if queue_size < 1 {
                    return Err(ErrorKind::MinChannelSizeOne.into());
                }
            }

            _ => {}
        }

        let (events, sender) = Events::new(options);

        // Try to reuse a free slot
        //
        if let Some(i) = self.free_slots.pop() {
            self.observers[i] = Some(sender);
        } else {
            self.observers.push(Some(sender));
        }

        Ok(events)
    }
}

// See the documentation on Channel for how poll functions work for the channels we use.
//
impl<Event> Sink<Event> for Pharos<Event>
where
    Event: Clone + 'static + Sync + Send,
{
    type Error = Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.state == State::Closed {
            return Err(ErrorKind::Closed.into()).into();
        }

        // As soon as any is not ready, we are not ready
        //
        for obs in self.get_mut().observers.iter_mut() {
            if let Some(ref mut o) = obs {
                let res = ready!(Pin::new(o).poll_ready(cx));

                // Errors mean disconnected, so drop.
                //
                if res.is_err() {
                    *obs = None;
                }
            }
        }

        Ok(()).into()
    }

    fn start_send(self: Pin<&mut Self>, evt: Event) -> Result<(), Self::Error> {
        if self.state == State::Closed {
            return Err(ErrorKind::Closed.into());
        }

        let this = self.get_mut();

        for (i, opt) in this.observers.iter_mut().enumerate() {
            // if this spot in the vector has a sender
            //
            if let Some(obs) = opt {
                // if it's closed, let's remove it.
                //
                if obs.is_closed() {
                    this.free_slots.push(i);

                    *opt = None;
                }
                // else if it is interested in this event
                //
                else if obs.filter(&evt) {
                    // if sending fails, remove it
                    //
                    if Pin::new(obs).start_send(evt.clone()).is_err() {
                        this.free_slots.push(i);

                        *opt = None;
                    }
                }
            }
        }

        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.state == State::Closed {
            return Err(ErrorKind::Closed.into()).into();
        }

        // We loop over all, polling them all. If any return pending, we return pending.
        // If any return an error, we drop them.
        //
        let mut pending = false;
        let this = self.get_mut();

        for (i, opt) in this.observers.iter_mut().enumerate() {
            if let Some(ref mut obs) = opt {
                match Pin::new(obs).poll_flush(cx) {
                    Poll::Pending => pending = true,
                    Poll::Ready(Ok(_)) => continue,

                    Poll::Ready(Err(_)) => {
                        this.free_slots.push(i);

                        *opt = None;
                    }
                }
            }
        }

        if pending {
            Poll::Pending
        } else {
            Ok(()).into()
        }
    }

    /// Will close and drop all observers. The pharos object will remain operational however.
    /// The main annoyance would be that we'd have to make
    //
    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.state == State::Closed {
            return Ok(()).into();
        } else {
            self.state = State::Closed;
        }

        let this = self.get_mut();

        for (i, opt) in this.observers.iter_mut().enumerate() {
            if let Some(ref mut obs) = opt {
                let res = ready!(Pin::new(obs).poll_close(cx));

                if res.is_err() {
                    this.free_slots.push(i);

                    *opt = None;
                }
            }
        }

        Ok(()).into()
    }
}

#[cfg(test)]
//
mod tests {
    // Tested:
    //
    // - ✔ debug impl shows generic type
    // - ✔ storage length and free slots bookkeeping
    // - ✔ observe: we actually reuse free slots
    // - ✔ observe: cannot observe after calling close
    // - ✔ observe: refuse Channel::Bounded(0)
    // - ✔ poll_ready have a channel that is full, verify we return pending.
    // - ✔ poll_ready have a channel that is disconnected, verify we drop it.
    // - ✔ poll_ready should return closed if the pharos is closed.
    // - ✔ start_send verify message arrives
    // - ✔ start_send drop disconnected channel
    // - ✔ start_send filter message
    // - ✔ poll_flush drop on error
    //
    use crate::{import::*, *};

    #[test]
    //
    fn debug() {
        let lighthouse = Pharos::<bool>::default();

        assert_eq!("pharos::Pharos<bool>", &format!("{:?}", lighthouse));
    }

    // #[test]
    // //
    // fn size_of_sender()
    // {
    // 	dbg!( std::mem::size_of::<Option<Sender<bool>>>() );
    // 	dbg!( std::mem::size_of::<Events<bool>>() );
    // }

    // verify storage_len and num_observers
    //
    #[test]
    //
    fn new() {
        let ph = Pharos::<bool>::new(5);

        assert_eq!(ph.observers.capacity(), 5);
    }

    // verify storage_len and num_observers
    //
    #[test]
    //
    fn storage_len() {
        let mut ph = Pharos::<bool>::default();

        assert_eq!(ph.storage_len(), 0);
        assert_eq!(ph.num_observers(), 0);
        assert_eq!(ph.free_slots.len(), 0);

        let mut a = ph.observe(ObserveConfig::default()).expect("observe");

        assert_eq!(ph.storage_len(), 1);
        assert_eq!(ph.num_observers(), 1);
        assert_eq!(ph.free_slots.len(), 0);

        let b = ph.observe(ObserveConfig::default()).expect("observe");

        assert_eq!(ph.storage_len(), 2);
        assert_eq!(ph.num_observers(), 2);
        assert_eq!(ph.free_slots.len(), 0);

        a.close();

        assert_eq!(ph.storage_len(), 2);
        assert_eq!(ph.num_observers(), 1);
        assert_eq!(&ph.free_slots, &[0]);

        drop(b);

        assert_eq!(ph.storage_len(), 2);
        assert_eq!(ph.num_observers(), 0);
        assert_eq!(&ph.free_slots, &[0, 1]);
    }

    // observe: Make sure we are reusing slots
    //
    #[test]
    //
    fn reuse() {
        let mut ph = Pharos::<bool>::default();
        let _a = ph.observe(ObserveConfig::default());
        let b = ph.observe(ObserveConfig::default());
        let _c = ph.observe(ObserveConfig::default());

        assert_eq!(ph.storage_len(), 3);
        assert_eq!(ph.num_observers(), 3);

        drop(b);

        // It's important we call num_observers here, to clear the dropped one
        //
        assert_eq!(ph.storage_len(), 3);
        assert_eq!(ph.num_observers(), 2);

        assert!(ph.observers[1].is_none());
        assert_eq!(&ph.free_slots, &[1]);

        let _d = ph.observe(ObserveConfig::default());

        assert_eq!(ph.storage_len(), 3);
        assert_eq!(ph.num_observers(), 3);
        assert_eq!(ph.free_slots.len(), 0);

        let _e = ph.observe(ObserveConfig::default());

        // Now we should have pushed again
        //
        assert_eq!(ph.storage_len(), 4);
        assert_eq!(ph.num_observers(), 4);
        assert_eq!(ph.free_slots.len(), 0);
    }

    // observe: verify we can no longer observe after calling close
    //
    #[test]
    //
    fn observe_after_close() {
        let mut ph = Pharos::<bool>::default();

        block_on(ph.close()).expect("close");

        let res = ph.observe(ObserveConfig::default());

        assert!(res.is_err());
        assert_eq!(ErrorKind::Closed, res.unwrap_err().kind());
    }

    // observe: refuse Channel::Bounded(0)
    //
    #[test]
    //
    fn observe_refuse_zero() {
        let mut ph = Pharos::<bool>::default();

        let res = ph.observe(Channel::Bounded(0).into());

        assert!(res.is_err());
        assert_eq!(ErrorKind::MinChannelSizeOne, res.unwrap_err().kind());
    }

    // verify that one observer blocks pharos.
    //
    #[test]
    //
    fn poll_ready_pending() {
        block_on(poll_fn(move |mut cx| {
            let mut ph = Pharos::default();

            let _open = ph.observe(Channel::Bounded(10).into()).expect("observe");
            let mut full = ph.observe(Channel::Bounded(1).into()).expect("observe");
            let _unbound = ph.observe(Channel::Unbounded.into()).expect("observe");

            let mut ph = Pin::new(&mut ph);

            assert_matches!(ph.as_mut().poll_ready(&mut cx), Poll::Ready(Ok(_)));
            assert!(ph.as_mut().start_send(true).is_ok());

            assert_matches!(ph.as_mut().poll_ready(&mut cx), Poll::Pending);

            assert_eq!(Pin::new(&mut full).poll_next(cx), Poll::Ready(Some(true)));

            assert_matches!(ph.as_mut().poll_ready(&mut cx), Poll::Ready(Ok(_)));

            ().into()
        }));
    }

    // pharos drops closed observers.
    //
    #[test]
    //
    fn poll_ready_drop() {
        block_on(poll_fn(move |mut cx| {
            let mut ph = Pharos::<bool>::default();

            let _open = ph.observe(Channel::Bounded(10).into()).expect("observe");
            let full = ph.observe(Channel::Bounded(1).into()).expect("observe");
            let _unbound = ph.observe(Channel::Unbounded.into()).expect("observe");

            let mut ph = Pin::new(&mut ph);

            drop(full);

            assert_matches!(ph.as_mut().poll_ready(&mut cx), Poll::Ready(Ok(_)));

            assert!(ph.observers[1].is_none());
            ().into()
        }));
    }

    // poll_ready should return closed if the pharos is closed.
    //
    #[test]
    //
    fn poll_ready_closed() {
        block_on(poll_fn(move |mut cx| {
            let mut ph = Pharos::<bool>::default();

            let mut ph = Pin::new(&mut ph);

            assert_matches!(ph.as_mut().poll_close(cx), Poll::Ready(Ok(())));

            let res = ph.as_mut().poll_ready(&mut cx);

            assert_matches!(res, Poll::Ready(Err(_)));

            match res {
                Poll::Ready(Err(e)) => assert_eq!(ErrorKind::Closed, e.kind()),
                _ => assert!(false, "wrong result "),
            }

            ().into()
        }));
    }

    // start_send verify message arrives.
    //
    #[test]
    //
    fn start_send_arrive() {
        block_on(poll_fn(move |mut cx| {
            let mut ph = Pharos::default();

            let _open = ph.observe(Channel::Bounded(10).into()).expect("observe");
            let mut full = ph.observe(Channel::Bounded(1).into()).expect("observe");
            let _unbound = ph.observe(Channel::Unbounded.into()).expect("observe");

            let mut ph = Pin::new(&mut ph);

            assert_matches!(ph.as_mut().poll_ready(&mut cx), Poll::Ready(Ok(_)));
            assert!(ph.as_mut().start_send(3).is_ok());

            assert_eq!(Pin::new(&mut full).poll_next(cx), Poll::Ready(Some(3)));

            ().into()
        }));
    }

    // pharos drops closed observers.
    //
    #[test]
    //
    fn poll_flush_drop() {
        block_on(poll_fn(move |mut cx| {
            let mut ph = Pharos::<bool>::default();

            let _open = ph.observe(Channel::Bounded(10).into()).expect("observe");
            let full = ph.observe(Channel::Bounded(1).into()).expect("observe");
            let _unbound = ph.observe(Channel::Unbounded.into()).expect("observe");

            let mut ph = Pin::new(&mut ph);

            drop(full);

            assert_matches!(ph.as_mut().poll_flush(&mut cx), Poll::Ready(Ok(_)));

            assert!(ph.observers[1].is_none());
            ().into()
        }));
    }
}
