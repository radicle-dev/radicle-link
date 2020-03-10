use std::hash;

mod thread;
pub use thread::{Error as ThreadError, Replies, ReplyTo, Status, Thread};

pub struct Issue<Author> {
    pub author: Author,
    pub title: String,
    pub thread: Thread<Comment<Author>>,
    pub meta: (), // TODO(fintan): fill in meta data
}

impl<Author> Issue<Author> {
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
    pub fn new(author: Author, title: String, content: String) -> Self
    where
        Author: Clone + Eq,
    {
        let comment = Comment {
            author: author.clone(),
            content,
        };

        Issue {
            author,
            title,
            thread: Thread::new(comment),
            meta: (),
        }
    }

    pub fn reply(&mut self, comment: Comment<Author>, reply_to: ReplyTo) {
        self.thread.reply(comment, reply_to)
    }
}

#[derive(Debug, PartialEq, Eq, hash::Hash)]
pub struct Comment<Author> {
    pub author: Author,
    pub content: String,
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
