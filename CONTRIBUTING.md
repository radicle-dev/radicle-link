Thank you for your interest in contributing to this project!

Before you submit your first patch, please take a moment to review the following
guidelines:

## Certificate of Origin

By contributing to this project, you agree to the [Developer Certificate of
Origin (DCO)][dco]. This document was created by the Linux Kernel community
and is a simple statement that you, as a contributor, have the legal right to
make the contribution.

In order to show your agreement with the DCO you should include at the end of
the commit message, the following line:

    Signed-off-by: John Doe <john.doe@example.com>

using your real name and email.

This can be done easily using `git commit -s`.

### Fixing the DCO

If you did not sign-off one or more of your commits then it is not all for not.
The crowd at `src-d` have a [wonderful guide][fixing-dco] on how to remedy this
situation.

## License Header

As part of our license, we must include a license header at the top of each
source file. The template for this header can be found [here][header-template].
If you are creating a new file in the repository you will have to add this
header to the file.

# Submitting Changes

We decided to experiment with using mailing lists as the vehicle for
submitting and discussing changes. This decision was documented in:

* [Submitting Patches to radicle-link][submit-patch]
* [radicle-link Maintainers Guide][maintainers-guide]

If you are interested in submitting some changes to `radicle-link`, we
kindly ask that you first read the [guidelines][submit-patch].

The simplest way to contribute is by using the [patch] script to
create the patch, e.g.

```
$ git remote add me https://github.com/FintanH/radicle-link
$ cd scripts/contributing
$ ./patch me update-docs v1 "https://github.com/FintanH/radicle-link"
```

For maintainers, you are expected to sign the tag you are
creating. The final command can be used as follows:

```
$ ./patch me update-docs v1 "https://github.com/FintanH/radicle-link" sign
```

If this is the first version of the patch, and you have the latest
`master` up-to-date, you can generate the `*.patch` files by running:

```
$ git format-patch --cover-letter origin/master..HEAD --to "~radicle-link/dev@lists.sr.ht" -v1
```

You can then edit the `v1-0000-cover-letter.patch` file by replacing
`**SUBJECT** and `**BLURB HERE**`, as well as adding the
`Published-as` trailer.

The final step is to send the patch series to the mailing list by
running:

```
$ git send-email *.patch
```

## Modifying the specs

When any of the spec files is modified, e.g. one from `docs/spec/sections`,
the docs need to be re-rendered and the files in `docs/spec/out` need to be
updated. To do this, run `scripts/render-docs`.

[dco]: ./DCO
[fixing-dco]: https://github.com/src-d/guide/blob/master/developer-community/fix-DCO.md
[header-template]: ./.license-header
[maintainers-guide]: https://github.com/radicle-dev/radicle-link/blob/master/docs/maintainers-guide.adoc
[patch]: ./scripts/contributing/patch
[submit-patch]: https://github.com/radicle-dev/radicle-link/blob/master/docs/submitting-patches.adoc
