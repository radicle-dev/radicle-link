// This file is part of radicle-link
// <https://github.com/radicle-dev/radicle-link>
//
// Copyright (C) 2019-2020 The Radicle Team <dev@radicle.xyz>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License version 3 or
// later as published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

use crate::{
    comment::Comment,
    issue::Issue,
    metadata::{Label, Reaction, Title},
    thread::{AppendTo, Finger, ReplyFinger},
};
use pretty_assertions::assert_eq;

#[test]
fn folding_ops() {
    let mut global_comment_id = 0;
    let mut new_comment_id = || {
        let new_id = global_comment_id;
        global_comment_id += 1;
        new_id
    };

    let initial_comment_id = new_comment_id();

    let mut issue = Issue::new(
        1,
        initial_comment_id,
        "fintohaps",
        Title::from("Issue Ops"),
        "This is how we create issues in radicle".to_string(),
    );

    let mut expected_issue = issue.clone();
    let title_change = expected_issue.replace_title(Title::from("Issue Ops - Origami"));
    let assign_massi = expected_issue.assign("mmassi");
    let label_collab = expected_issue.label(Label::from("collab"));
    let replace_description = expected_issue
        .with_comments(|thread| {
            thread.edit(Finger::Root, |comment| {
                Ok(comment
                    .replace_content(
                        "fintohaps",
                        "This is how we create issues in radicle: by folding over operations."
                            .to_string(),
                    )
                    .unwrap())
            })
        })
        .expect("Failed to edit comment");
    let kim_comment = expected_issue
        .with_comments(|thread| {
            thread.append(
                AppendTo::Main,
                Comment::new(
                    new_comment_id(),
                    "kim",
                    "Are these operations CRDTs?".to_string(),
                ),
            )
        })
        .expect("kim failed to append comment");
    let finto_reply_to_kim = expected_issue.with_comments(|thread| {
        thread.append(AppendTo::Thread(0),
        Comment::new(
            new_comment_id(),
            "fintohaps",
            "Not quite, they look similar but they hold less state and causality happens locally.".to_string()))
    }).expect("fintohaps reply to thread failed");
    let xla_comment = expected_issue
        .with_comments(|thread| {
            thread.append(
                AppendTo::Main,
                Comment::new(
                    new_comment_id(),
                    "xla",
                    "Let's integrate! :rocket:".to_string(),
                ),
            )
        })
        .expect("xla comment failed");
    let finto_reaction_to_xla = expected_issue
        .with_comments(|thread| {
            thread.edit(Finger::Reply(ReplyFinger::Main(1)), |comment| {
                Ok(comment.react(Reaction {
                    user: "fintohaps",
                    value: ":heart".to_string(),
                }))
            })
        })
        .expect("fintohaps failed to react to xla");
    let finto_edit_comment = expected_issue
        .with_comments(|thread| {
            thread.edit(
                Finger::Reply(ReplyFinger::Thread { main: 0, reply: 0 }),
                |comment| {
                    Ok(comment
                    .replace_content(
                        "fintohaps",
                        "They look similar but they hold less state and causality happens locally."
                            .to_string(),
                    )
                    .expect("only fintohaps can edit his own comment"))
                },
            )
        })
        .expect("fintohaps failed to edit his comment");

    let ops = vec![
        title_change,
        assign_massi,
        label_collab,
        replace_description,
        kim_comment,
        xla_comment,
        finto_reply_to_kim,
        finto_reaction_to_xla,
        finto_edit_comment,
    ];

    issue
        .fold_issue(ops.into_iter())
        .expect("Folding of ops failed");

    assert_eq!(expected_issue, issue);
}
