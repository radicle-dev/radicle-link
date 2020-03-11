#![deny(missing_docs, unused_import_braces, unused_qualifications, warnings)]
#![feature(vec_remove_item)]
use std::hash::Hash;

mod thread;
pub use thread::{Error as ThreadError, Replies, ReplyTo, Status, Thread};

mod metadata;
pub use metadata::*;

#[derive(Debug, Clone)]
pub struct Issue<IssueId, CommentId, User: Eq + Hash> {
    identifier: IssueId,
    author: User,
    title: String,
    thread: Thread<Comment<CommentId, User>>,
    meta: Metadata<User>,
}

impl<IssueId, CommentId, User: Eq + Hash> Issue<IssueId, CommentId, User> {
    /// ```
    /// use radicle_tracker::Issue;
    ///
    /// let issue = Issue::new(
    ///     String::from("Monadic"),
    ///     String::from("Buggy Boeuf"),
    ///     String::from("We have bugs in our boeuf"),
    /// );
    ///
    /// assert_eq!(issue.author, String::from("Monadic"));
    /// assert_eq!(issue.title, String::from("Buggy Boeuf"));
    /// assert_eq!(issue.thread.view().unwrap().get().content, String::from("We have bugs in our boeuf"));
    /// assert!(issue.thread.view().unwrap().get().author == issue.author);
    /// assert_eq!(issue.meta, ());
    /// ```
    pub fn new(
        identifier: IssueId,
        comment_id: CommentId,
        author: User,
        title: String,
        content: String,
    ) -> Self
    where
        User: Clone + Eq,
    {
        let comment = Comment::new(comment_id, author.clone(), content);

        Issue {
            identifier,
            author,
            title,
            thread: Thread::new(comment),
            meta: Metadata::new(),
        }
    }

    pub fn reply(&mut self, comment: Comment<CommentId, User>, reply_to: ReplyTo) {
        self.thread.reply(comment, reply_to)
    }
}
