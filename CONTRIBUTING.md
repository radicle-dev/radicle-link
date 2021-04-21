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

## Modifying the specs

When any of the spec files is modified, e.g. one from `docs/spec/sections`,
the docs need to be re-rendered and the files in `docs/spec/out` need to be
updated. To do this, run `scripts/render-docs`.

[dco]: ./DCO
[fixing-dco]: https://docs.github.com/en/free-pro-team@latest/github/building-a-strong-community/creating-a-pull-request-template-for-your-repository
[header-template]: ./.license-header-template
