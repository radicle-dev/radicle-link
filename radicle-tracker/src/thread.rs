use either::Either;
use nonempty::NonEmpty;
use thiserror::Error;

/// The "liveness" status of some data.
///
/// The data can be:
///     * `Live` and so it has only been created.
///     * `Dead` and so it was created and deleted.
///
/// TODO: we may want to consider `Modified`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status<A> {
    Live(A),
    Dead(A),
}

impl<A> Status<A> {
    /// Mark the status as `Dead`, no matter what the original status was.
    fn kill(&mut self)
    where
        A: Clone,
    {
        *self = Status::Dead(self.get().clone())
    }

    /// Get the reference to the value inside the status.
    pub fn get(&self) -> &A {
        match self {
            Status::Live(a) => a,
            Status::Dead(a) => a,
        }
    }

    /// Get the mutable reference to the value inside the status.
    fn get_mut(&mut self) -> &mut A {
        match self {
            Status::Live(a) => a,
            Status::Dead(a) => a,
        }
    }

    /// If the status is `Live` then return a reference to it.
    pub fn live(&self) -> Option<&A> {
        match self {
            Status::Live(a) => Some(a),
            _ => None,
        }
    }

    /// If the status is `Dead` then return a reference to it.
    pub fn dead(&self) -> Option<&A> {
        match self {
            Status::Dead(a) => Some(a),
            _ => None,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    #[error("Tried to move to previous item in the main thread, but we are at the first")]
    PreviousMainOutOfBounds,
    #[error("Cannot move to previous item in a thread when we are located on the main thread")]
    PreviousThreadOnMain,
    #[error("Tried to move to next item in the main thread, but we are at the last")]
    NextMainOutOfBounds,
    #[error("Tried to move to next item in the reply thread, but we are at the last")]
    NextRepliesOutOfBound,
    #[error("Cannot delete the main item of the thread")]
    DeleteFirstMain,
}

/// A collection of replies where a reply is any item that has a [`Status`].
///
/// `Replies` are deliberately opaque as they should mostly be interacted with
/// via [`Thread`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replies<A>(Vec<Status<A>>);

impl<A> Replies<A> {
    fn new() -> Self {
        Replies(vec![])
    }

    fn reply(&mut self, a: A) {
        self.0.push(Status::Live(a))
    }

    fn delete(&mut self, index: usize) -> Option<&mut Status<A>>
    where
        A: Clone,
    {
        let node = self.0.get_mut(index)?;

        node.kill();

        Some(node)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    fn get(&self, index: usize) -> Option<&Status<A>> {
        self.0.get(index)
    }

    fn get_mut(&mut self, index: usize) -> Option<&mut Status<A>> {
        self.0.get_mut(index)
    }

    pub fn iter<'a>(&'a self) -> impl Iterator<Item = &Status<A>> + 'a {
        self.0.iter()
    }
}

// This point to the main thread, and the first item in that thread.
const ROOT_FINGER: Either<usize, (usize, usize)> = Either::Left(0);

/// A `Thread` is non-empty series of items and replies to those items.
///
/// TODO: This doesn't correctly capture the design we want. Technically it
/// should just be a single comment at the top, followed by a series of
/// "threads".
#[derive(Debug, Clone)]
pub struct Thread<A> {
    // A finger points into the `main_thread` structure.
    // If it is `Left` then it is pointing to the main thread.
    // If it is `Right` then it is pointing to a reply to a comment in the main thread.
    _finger: Either<usize, (usize, usize)>,
    main_thread: NonEmpty<(Status<A>, Replies<A>)>,
}

impl<A: PartialEq> PartialEq for Thread<A> {
    fn eq(&self, other: &Self) -> bool {
        self.main_thread == other.main_thread
    }
}

/// `ReplyTo` tells the navigation and reply functions whether they should take
/// action on the "main thread" or on a "reply thread".
///
/// See [`Thread::reply`] for an example of how it is used.
pub enum ReplyTo {
    Main,
    Thread,
}

impl<A> Thread<A> {
    /// Create a new `Thread` with `a` as the root of the `Thread`.
    ///
    /// # Examples
    ///
    /// ```
    /// use radicle_tracker::{Status, Thread};
    ///
    /// let (thread) = Thread::new(String::from("Discussing rose trees"));
    ///
    /// assert_eq!(thread.view(), Ok(&Status::Live(String::from("Discussing rose trees"))));
    /// ```
    pub fn new(a: A) -> Self {
        Thread {
            _finger: ROOT_FINGER,
            main_thread: NonEmpty::new((Status::Live(a), Replies::new())),
        }
    }

    /// Look at the previous reply of the thread. If it's the case that we are
    /// looking at the first reply to an item on the main thread, then we
    /// will point to the main thread item.
    ///
    /// # Errors
    ///
    /// The function will fail with:
    ///     * [`Error::PreviousMainOutOfBounds`] if we are looking at the start
    ///       of the main thread.
    ///     * [`Error::PreviousThreadOnMain`] if we use [`ReplyTo::Thread`]
    ///       while on the main
    ///     thread.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// #
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use radicle_tracker::{ReplyTo, Status, Thread};
    ///
    /// let (mut thread) = Thread::new(String::from("Discussing rose trees"));
    ///
    /// // Reply to the main thread
    /// thread.reply(String::from("I love rose trees!"), ReplyTo::Main);
    ///
    /// thread.previous_reply(ReplyTo::Main)?;
    /// assert_eq!(thread.view(), Ok(&Status::Live(String::from("Discussing rose trees"))));
    /// #
    /// #     Ok(())
    /// # }
    /// ```
    ///
    /// ```
    /// # use std::error::Error;
    /// #
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use radicle_tracker::{ReplyTo, Status, Thread};
    ///
    /// let (mut thread) = Thread::new(String::from("Discussing rose trees"));
    ///
    /// // Reply to the main thread
    /// thread.reply(String::from("I love rose trees!"), ReplyTo::Main);
    ///
    /// // Reply to the 1st comment
    /// thread.reply(String::from("Is this about flowers?"), ReplyTo::Thread);
    ///
    /// assert_eq!(thread.view(), Ok(&Status::Live(String::from("Is this about flowers?"))));
    ///
    /// thread.previous_reply(ReplyTo::Thread)?;
    /// assert_eq!(thread.view(), Ok(&Status::Live(String::from("I love rose trees!"))));
    /// #
    /// #     Ok(())
    /// # }
    /// ```
    pub fn previous_reply(&mut self, reply_to: ReplyTo) -> Result<(), Error> {
        match self._finger.as_mut() {
            Either::Left(main_ix) if *main_ix == 0 => Err(Error::PreviousMainOutOfBounds),
            Either::Left(main_ix) => match reply_to {
                ReplyTo::Main => {
                    *main_ix -= 1;
                    Ok(())
                },
                ReplyTo::Thread => Err(Error::PreviousThreadOnMain),
            },
            Either::Right((main_ix, replies_ix)) => match reply_to {
                ReplyTo::Main => {
                    if *main_ix == 0 {
                        return Err(Error::PreviousMainOutOfBounds);
                    }

                    self._finger = Either::Left(*main_ix - 1);
                    Ok(())
                },
                ReplyTo::Thread => {
                    // If we're at the first reply, then we move to the main thread.
                    if *replies_ix == 0 {
                        self._finger = Either::Left(*main_ix);
                    } else {
                        *replies_ix -= 1;
                    }
                    Ok(())
                },
            },
        }
    }

    /// Look at the next reply of the thread.
    ///
    /// # Errors
    ///
    /// The function will fail with:
    ///     * [`Error::NextMainOutOfBounds`] if we are at the end of the main
    ///       thread and attempt to go to the next item.
    ///     * [`Error::NextRepliesOutOfBound`] if we are at the end of the reply
    ///       thread and attempt to go to the next item.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// #
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use radicle_tracker::{ReplyTo, Status, Thread};
    ///
    /// let (mut thread) = Thread::new(String::from("Discussing rose trees"));
    ///
    /// // Reply to the main thread
    /// thread.reply(String::from("I love rose trees!"), ReplyTo::Main);
    ///
    /// thread.root();
    /// thread.next_reply(ReplyTo::Main)?;
    /// assert_eq!(thread.view(), Ok(&Status::Live(String::from("I love rose trees!"))));
    /// #
    /// #     Ok(())
    /// # }
    /// ```
    pub fn next_reply(&mut self, reply_to: ReplyTo) -> Result<(), Error> {
        let replies_count = self.replies_count();
        let main_bound = self.main_thread.len() - 1;

        let replies_bound = if replies_count == 0 {
            None
        } else {
            Some(replies_count - 1)
        };

        match self._finger.as_mut() {
            Either::Left(main_ix) => match reply_to {
                ReplyTo::Main => {
                    if *main_ix == main_bound {
                        return Err(Error::NextMainOutOfBounds);
                    }

                    *main_ix += 1;
                    Ok(())
                },
                ReplyTo::Thread => match replies_bound {
                    None => Err(Error::NextRepliesOutOfBound),
                    // We're ensuring that we have replies
                    Some(_) => {
                        self._finger = Either::Right((*main_ix, 0));
                        Ok(())
                    },
                },
            },
            Either::Right((main_ix, replies_ix)) => match reply_to {
                ReplyTo::Main => {
                    if *main_ix == main_bound {
                        return Err(Error::NextMainOutOfBounds);
                    }

                    self._finger = Either::Left(*main_ix + 1);
                    Ok(())
                },
                ReplyTo::Thread => match replies_bound {
                    None => Err(Error::NextRepliesOutOfBound),
                    Some(bound) => {
                        if *replies_ix == bound {
                            return Err(Error::NextRepliesOutOfBound);
                        } else {
                            *replies_ix += 1;
                        }
                        Ok(())
                    },
                },
            },
        }
    }

    /// Look at the root of the thread.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// #
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use radicle_tracker::{ReplyTo, Status, Thread};
    ///
    /// let (mut thread) = Thread::new(String::from("Discussing rose trees"));
    ///
    /// // Reply to the main thread
    /// thread.reply(String::from("I love rose trees!"), ReplyTo::Main);
    ///
    /// thread.root();
    /// assert_eq!(thread.view(), Ok(&Status::Live(String::from("Discussing rose trees"))));
    /// #
    /// #     Ok(())
    /// # }
    /// ```
    pub fn root(&mut self) {
        self._finger = ROOT_FINGER;
    }

    /// Reply to the thread. Depending on what type of [`ReplyTo`] value we pass
    /// we will either reply to the main thread or we will reply to the
    /// reply thread.
    ///
    /// Once we have replied we will be pointing to the latest reply, whether it
    /// is on the main thread or the reply thread.
    ///
    /// # Panics
    ///
    /// If the internal finger into the thread is out of bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// #
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use radicle_tracker::{ReplyTo, Status, Thread};
    ///
    /// let (mut thread) = Thread::new(String::from("Discussing rose trees"));
    ///
    /// // Reply to the main thread
    /// thread.reply(String::from("I love rose trees!"), ReplyTo::Main);
    ///
    /// // Reply to the 1st comment on the main thread
    /// thread.reply(
    ///     String::from("Did you know rose trees are equivalent to Cofree []?"),
    ///     ReplyTo::Thread
    /// );
    ///
    /// thread.reply(String::from("What should we use them for?"), ReplyTo::Main);
    ///
    /// thread.root();
    /// assert_eq!(thread.view(), Ok(&Status::Live(String::from("Discussing rose trees"))));
    ///
    /// thread.next_reply(ReplyTo::Main)?;
    /// assert_eq!(thread.view(), Ok(&Status::Live(String::from("I love rose trees!"))));
    ///
    /// thread.next_reply(ReplyTo::Main)?;
    /// assert_eq!(thread.view(), Ok(&Status::Live(String::from("What should we use them for?"))));
    ///
    /// thread.previous_reply(ReplyTo::Main)?;
    /// thread.next_reply(ReplyTo::Thread)?;
    /// assert_eq!(
    ///     thread.view(),
    ///     Ok(&Status::Live(String::from("Did you know rose trees are equivalent to Cofree []?")))
    /// );
    /// #
    /// #     Ok(())
    /// # }
    /// ```
    pub fn reply(&mut self, a: A, reply_to: ReplyTo) {
        match self._finger {
            Either::Left(main_ix) => match reply_to {
                ReplyTo::Main => self.reply_main(a),
                ReplyTo::Thread => self.reply_thread(main_ix, a),
            },
            Either::Right((main_ix, _)) => match reply_to {
                ReplyTo::Main => self.reply_main(a),
                ReplyTo::Thread => self.reply_thread(main_ix, a),
            },
        }
    }

    /// Delete the item that we are looking at. This does not remove the item
    /// from the thread but rather marks it as [`Status::Dead`].
    ///
    /// # Panics
    ///
    /// If the internal finger into the thread is out of bounds.
    ///
    /// # Error
    ///
    /// Fails with [`Error::DeleteFirstMain`] if we attempt to delete the first
    /// item in the main thread.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// #
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use radicle_tracker::{ReplyTo, Status, Thread, ThreadError};
    ///
    /// let mut thread = Thread::new(String::from("Discussing rose trees"));
    ///
    /// // Reply to the main thread
    /// thread.reply(String::from("I love rose trees!"), ReplyTo::Main);
    ///
    /// // Reply to the 1st comment on the main thread
    /// thread.reply(
    ///     String::from("Did you know rose trees are equivalent to Cofree []?"),
    ///     ReplyTo::Thread
    /// );
    ///
    /// thread.reply(String::from("What should we use them for?"), ReplyTo::Main);
    ///
    /// // Delete the last comment on the main thread
    /// thread.delete();
    ///
    /// thread.root();
    /// assert_eq!(thread.view(), Ok(&Status::Live(String::from("Discussing rose trees"))));
    ///
    /// thread.next_reply(ReplyTo::Main)?;
    /// assert_eq!(thread.view(), Ok(&Status::Live(String::from("I love rose trees!"))));
    ///
    /// thread.next_reply(ReplyTo::Main)?;
    /// assert_eq!(thread.view(), Ok(&Status::Dead(String::from("What should we use them for?"))));
    /// #
    /// #     Ok(())
    /// # }
    /// ```
    pub fn delete(&mut self) -> Result<(), Error>
    where
        A: Clone,
    {
        match self._finger {
            Either::Left(main_ix) if main_ix == 0 => Err(Error::DeleteFirstMain),
            Either::Left(main_ix) => {
                let (node, _) = self.index_main_mut(main_ix);
                node.kill();
                Ok(())
            },
            Either::Right((main_ix, replies_ix)) => {
                let (_, replies) = self.index_main_mut(main_ix);
                replies
                    .delete(replies_ix)
                    .unwrap_or_else(|| panic!("Reply index is out of bounds: {}", replies_ix));
                Ok(())
            },
        }
    }

    /// Edit the item we are looking at with the function `f`.
    ///
    /// # Panics
    ///
    /// If the internal finger into the thread is out of bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// #
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use radicle_tracker::{ReplyTo, Status, Thread, ThreadError};
    ///
    /// let mut thread = Thread::new(String::from("Discussing rose trees"));
    ///
    /// // Reply to the main thread
    /// thread.reply(String::from("I love rose trees!"), ReplyTo::Main);
    ///
    /// // Reply to the 1st comment on the main thread
    /// thread.reply(
    ///     String::from("Did you know rose trees are equivalent to Cofree []?"),
    ///     ReplyTo::Thread
    /// );
    ///
    /// thread.reply(String::from("What should we use them for?"), ReplyTo::Main);
    /// thread.edit(|body| *body = String::from("How can we use them?"));
    ///
    /// assert_eq!(thread.view(), Ok(&Status::Live(String::from("How can we use them?"))));
    /// #
    /// #     Ok(())
    /// # }
    /// ```
    pub fn edit<F>(&mut self, f: F) -> Result<(), Error>
    where
        F: FnOnce(&mut A) -> (),
    {
        match self._finger {
            Either::Left(main_ix) => {
                let (node, _) = self.index_main_mut(main_ix);
                f(node.get_mut());
                Ok(())
            },
            Either::Right((main_ix, replies_ix)) => {
                let (_, replies) = self.index_main_mut(main_ix);
                let node = replies
                    .get_mut(replies_ix)
                    .unwrap_or_else(|| panic!("Reply index is out of bounds: {}", replies_ix));
                f(node.get_mut());
                Ok(())
            },
        }
    }

    /// Expand the current main thread item we are looking at into the full
    /// non-empty view of items.
    ///
    /// # Panics
    ///
    /// If the internal finger into the thread is out of bounds.
    pub fn expand(&self) -> NonEmpty<Status<A>>
    where
        A: Clone,
    {
        let main_ix = match self._finger {
            Either::Left(main_ix) => main_ix,
            Either::Right((main_ix, _)) => main_ix,
        };

        let (node, replies) = self.index_main(main_ix);
        NonEmpty::from((node.clone(), replies.clone().0))
    }

    /// Look at the current item we are pointing to in the thread.
    ///
    /// # Panics
    ///
    /// If the internal finger into the thread is out of bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use radicle_tracker::{Status, Thread};
    ///
    /// let (thread) = Thread::new(String::from("Discussing rose trees"));
    ///
    /// assert_eq!(thread.view(), Ok(&Status::Live(String::from("Discussing rose trees"))));
    /// ```
    pub fn view(&self) -> Result<&Status<A>, Error> {
        match self._finger {
            Either::Left(main_ix) => Ok(&self.index_main(main_ix).0),
            Either::Right((main_ix, replies_ix)) => Ok(self
                .index_main(main_ix)
                .1
                .get(replies_ix)
                .unwrap_or_else(|| panic!("Reply index is out of bounds: {}", replies_ix))),
        }
    }

    fn index_main(&self, main_ix: usize) -> &(Status<A>, Replies<A>) {
        self.main_thread
            .get(main_ix)
            .unwrap_or_else(|| panic!("Main index is out of bounds: {}", main_ix))
    }

    fn index_main_mut(&mut self, main_ix: usize) -> &mut (Status<A>, Replies<A>) {
        self.main_thread
            .get_mut(main_ix)
            .unwrap_or_else(|| panic!("Main index is out of bounds: {}", main_ix))
    }

    fn replies(&self, index: usize) -> &Replies<A> {
        &self.main_thread.get(index).unwrap().1
    }

    fn replies_mut(&mut self, index: usize) -> &mut Replies<A> {
        &mut self.main_thread.get_mut(index).unwrap().1
    }

    fn replies_count(&self) -> usize {
        let main_ix = match self._finger {
            Either::Left(main_ix) => main_ix,
            Either::Right((main_ix, _)) => main_ix,
        };

        self.replies(main_ix).len()
    }

    fn reply_main(&mut self, a: A) {
        self.main_thread.push((Status::Live(a), Replies::new()));
        self._finger = Either::Left(self.main_thread.len() - 1);
    }

    fn reply_thread(&mut self, main_ix: usize, a: A) {
        let replies = self.replies_mut(main_ix);
        replies.reply(a);
        let replies_ix = replies.len() - 1;
        self._finger = Either::Right((main_ix, replies_ix));
    }

    #[cfg(test)]
    fn prune(&mut self)
    where
        A: Clone,
    {
        let mut thread = vec![];
        for (node, replies) in self.main_thread.iter() {
            if node.dead().is_some() {
                continue;
            }

            thread.push((
                node.clone(),
                Replies(
                    replies
                        .clone()
                        .0
                        .into_iter()
                        .filter(|node| node.live().is_some())
                        .collect(),
                ),
            ))
        }

        let main_thread = NonEmpty::from_slice(&thread).unwrap();
        self.main_thread = main_thread;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// forall a. Thread::new(a).view() === a
    fn prop_view_of_new<A: Eq + Clone>(a: A) -> bool {
        Thread::new(a.clone()).view() == Ok(&Status::Live(a))
    }

    /// { new_path = thread.reply(path, comment)?
    ///   thread.delete(new_path)
    /// } === thread
    fn prop_deleting_a_replied_comment_is_noop<A>(
        thread: &mut Thread<A>,
        a: A,
    ) -> Result<bool, Error>
    where
        A: std::fmt::Debug + Clone + PartialEq,
    {
        let old_thread = thread.clone();
        thread.reply(a, ReplyTo::Main);
        thread.delete()?;
        thread.prune();

        Ok(*thread == old_thread)
    }

    /// Thread::new(comment).delete(comment) === None
    fn prop_deleting_root_should_not_be_possible<A: Eq>(a: A) -> bool
    where
        A: Clone,
    {
        Thread::new(a).delete() == Err(Error::DeleteFirstMain)
    }

    /// Thread::new(comment).edit(f, comment) ===
    /// Thread::new(f(comment).unwrap())
    fn prop_new_followed_by_edit_is_same_as_editing_followed_by_new<A, F>(mut a: A, f: &F) -> bool
    where
        A: Eq + Clone,
        F: Fn(&mut A) -> (),
    {
        let mut lhs = Thread::new(a.clone());
        lhs.edit(f).expect("Edit failed");

        f(&mut a);
        let rhs = Thread::new(a.clone());

        lhs == rhs
    }

    /// let (thread, path) = Thread::new(a)
    /// => thread.view(path) == a
    fn prop_root_followed_by_view<A>(a: A) -> bool
    where
        A: Eq + Clone,
    {
        let thread = Thread::new(a.clone());
        *thread.view().unwrap() == Status::Live(a)
    }

    #[test]
    fn check_view_of_new() {
        assert!(prop_view_of_new("New thread"))
    }

    #[test]
    fn check_root_followed_by_view() {
        assert!(prop_root_followed_by_view("New thread"))
    }

    #[test]
    fn check_deleting_a_replied_comment_is_noop() -> Result<(), Error> {
        let mut thread = Thread::new("New thread");
        prop_deleting_a_replied_comment_is_noop(&mut thread, "New comment").map(|_| ())
    }

    #[test]
    fn check_deleting_root_should_not_be_possible() {
        assert!(prop_deleting_root_should_not_be_possible("New thread"))
    }

    #[test]
    fn check_new_followed_by_edit_is_same_as_editing_followed_by_new() {
        assert!(
            prop_new_followed_by_edit_is_same_as_editing_followed_by_new(
                String::from("new thread"),
                &|body| {
                    body.insert_str(0, "edit: ");
                }
            )
        )
    }
}
