.PHONY: all pdf html clean

default: all

all: pdf html
pdf: out/radicle-link.pdf
html: out/radicle-link.html

out:
	mkdir -p out

out/radicle-link.pdf: radicle-link-rev1-draft.md out pandoc/template.latex pandoc/ieee-with-url.csl references.bib
	pandoc \
		--standalone \
		--toc \
		--from=markdown \
		--template=pandoc/template.latex \
		--citeproc \
		--csl=pandoc/ieee-with-url.csl \
		-o $@ \
		radicle-link-rev1-draft.md

out/radicle-link.html: radicle-link-rev1-draft.md out/spec.css pandoc/template.html pandoc/ieee-with-url.csl references.bib
	pandoc \
		--standalone \
		--toc \
		--from=markdown \
		--template=pandoc/template.html \
		--metadata=pdfn:radicle-link.pdf \
		--css=spec.css \
		--mathjax \
		--citeproc \
		--csl=pandoc/ieee-with-url.csl \
		-o $@ \
		radicle-link-rev1-draft.md

out/spec.css: out pandoc/spec.css
	cp pandoc/spec.css out/spec.css

clean:
	rm -rf out
