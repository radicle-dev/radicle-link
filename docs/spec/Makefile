.PHONY: all pdf html clean

default: all

all: pdf html
pdf: out/radicle-link.pdf
html: out/radicle-link.html

sections := $(wildcard sections/*.md)

out:
	mkdir -p out

# FIXME: `--citeproc` produces LaTeX my texlive doesn't understand, so need to
# continue using `--filter=pandoc-citeproc` for PDF for now.
out/radicle-link.pdf: out meta.yaml $(sections) pandoc/template.latex pandoc/ieee-with-url.csl references.bib
	pandoc \
		--metadata-file=meta.yaml \
		--standalone \
		--toc \
		--from=markdown \
		--template=pandoc/template.latex \
		--filter=pandoc-citeproc \
		--csl=pandoc/ieee-with-url.csl \
		-o $@ \
		$$(<index)

out/radicle-link.html: out/spec.css meta.yaml $(sections) pandoc/template.html pandoc/ieee-with-url.csl references.bib
	pandoc \
		--metadata-file=meta.yaml \
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
		$$(cat index)

out/spec.css: out pandoc/spec.css
	cp pandoc/spec.css out/spec.css

clean:
	rm -rf out
