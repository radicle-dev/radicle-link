use std::{
    collections::{HashMap, HashSet},
    convert::Infallible,
    hash::Hash,
    str::FromStr,
};
use time::OffsetDateTime;

/// The metadata that is related to an issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata<User: Eq + Hash> {
    labels: HashSet<Label>,
    assignees: Assignees<User>,
}

impl<User: Eq + Hash> Metadata<User> {
    /// Initialise empty metadata.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Metadata {
            labels: HashSet::new(),
            assignees: Assignees::new(),
        }
    }

    /// Get a reference to the [`Label`]s in the metadata.
    pub fn labels(&self) -> &HashSet<Label> {
        &self.labels
    }

    /// Add a [`Label`] to the set of labels in the metadata.
    pub fn add_label(&mut self, label: Label) -> bool {
        self.labels.insert(label)
    }

    /// Remove a [`Label`] from the set of labels in the metadata.
    pub fn remove_label(&mut self, label: &Label) -> bool {
        self.labels.remove(label)
    }

    /// Get a reference to the [`Assignees`] in the metadata.
    pub fn assignees(&self) -> &Assignees<User> {
        &self.assignees
    }

    /// Add a `User` to the set of [`Assignees`] in the metadata.
    pub fn add_assignee(&mut self, assignee: User) -> bool {
        self.assignees.add(assignee)
    }

    /// Remove a `User` from the set of [`Assignees`] in the metadata.
    pub fn remove_assignee(&mut self, assignee: &User) -> bool {
        self.assignees.remove(assignee)
    }
}

/// The title of an issue.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Title(String);

impl Title {
    /// Create a new title.
    pub fn new(title: String) -> Self {
        Title(title)
    }
}

impl std::ops::Deref for Title {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<&str> for Title {
    fn from(s: &str) -> Self {
        Title(s.to_string())
    }
}

impl FromStr for Title {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Title(s.to_string()))
    }
}

/// A comment of an issue is composed of its [`Comment::author`], the
/// [`Comment::content`] of the comment, and its [`Comment::reactions`].
///
/// It has a unique identifier (of type `Id`) chosen by the implementor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment<Id, User: Eq + Hash> {
    identifier: Id,
    author: User,
    content: String,
    reactions: HashSet<Reaction<User>>,
    timestamp: OffsetDateTime,
}

impl<CommentId, User: Eq + Hash> Comment<CommentId, User> {
    /// Create a new `Comment`.
    pub fn new(identifier: CommentId, author: User, content: String) -> Self {
        let timestamp = OffsetDateTime::now_local();
        Self::new_with_timestamp(identifier, author, content, timestamp)
    }

    /// Create a new `Comment` with a supplied `timestamp`.
    pub fn new_with_timestamp(
        identifier: CommentId,
        author: User,
        content: String,
        timestamp: OffsetDateTime,
    ) -> Self {
        Comment {
            identifier,
            author,
            content,
            reactions: HashSet::new(),
            timestamp,
        }
    }

    /// Get a reference to to the author of this comment.
    pub fn author(&self) -> &User {
        &self.author
    }

    /// Get a reference to to the content of this comment.
    pub fn content(&self) -> &String {
        &self.content
    }

    /// Add a new reaction to the set of reactions on the comment.
    /// Returns `true` if the reaction was new.
    /// Returns `false` if the reaction already existed.
    pub fn react(&mut self, reaction: Reaction<User>) -> bool {
        self.reactions.insert(reaction)
    }

    /// Add a new reaction to the set of reactions on the comment.
    /// Returns `true` if the reaction was in the set and is now removed.
    /// Returns `false` if the reaction was not in the set to begin with.
    pub fn unreact(&mut self, reaction: &Reaction<User>) -> bool {
        self.reactions.remove(reaction)
    }

    /// Get the map of reactions to this comment.
    pub fn reactions(&self) -> HashMap<String, Vec<User>>
    where
        User: Clone,
    {
        let mut reaction_map = HashMap::new();
        for reaction in self.reactions.clone().into_iter() {
            reaction_map
                .entry(reaction.value.clone())
                .and_modify(|users: &mut Vec<User>| users.push(reaction.user.clone()))
                .or_insert_with(|| vec![reaction.user]);
        }
        reaction_map
    }
}

/// A custom label that can be added to an issue.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Label(String);

impl Label {
    /// Create a new `Label`.
    pub fn new(label: String) -> Self {
        Label(label)
    }
}

impl std::ops::Deref for Label {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromStr for Label {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Label(s.to_string()))
    }
}

/// A collection of users that represent the assigned users of the issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignees<User: Eq + Hash>(HashSet<User>);

impl<User: Eq + Hash> Assignees<User> {
    /// Create an empty set of assignees.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Assignees(HashSet::new())
    }

    /// Add a user to the set of assignees.
    /// If the set did not have this value present, `true` is returned.
    /// If the set did have this value present, `false` is returned.
    pub fn add(&mut self, user: User) -> bool {
        self.0.insert(user)
    }

    /// Remove the user from the set of assignees.
    /// If the set did have this value present, `true` is returned.
    /// If the set did not have this value present, `false` is returned.
    pub fn remove(&mut self, user: &User) -> bool {
        self.0.remove(user)
    }

    /// Check is the user is in the set of assignees.
    pub fn contains(&self, user: &User) -> bool {
        self.0.contains(user)
    }
}

impl<User: Eq + Hash> std::ops::Deref for Assignees<User> {
    type Target = HashSet<User>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A reaction is the pair of a user and a free-form reaction.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Reaction<User> {
    user: User,
    value: String,
}

impl<User> Reaction<User> {
    /// Create a new reaction.
    pub fn new(user: User, value: String) -> Self {
        Reaction { user, value }
    }

    /// Get the reference to the user of this reaction.
    pub fn user(&self) -> &User {
        &self.user
    }

    /// Get the reference to the value of this reaction.
    pub fn value(&self) -> &String {
        &self.value
    }
}
