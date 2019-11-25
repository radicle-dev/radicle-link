.PHONY: all pdf html clean

default: html


all: pdf html
pdf: out/radicle-link.pdf
html: out/radicle-link.html

out/%.pdf: %.md pandoc/template.latex pandoc/ieee-with-url.csl references.bib
	pandoc \
		$*.md \
		--standalone \
		--toc \
		--from=markdown \
		--template=pandoc/template.latex \
		--filter=pandoc-citeproc \
		--csl=pandoc/ieee-with-url.csl \
		-o $@

out/%.html: %.md pandoc/template.html out/spec.css pandoc/ieee-with-url.csl references.bib
	pandoc \
		$*.md \
		--standalone \
		--toc \
		--from=markdown \
		--template=pandoc/template.html \
		--metadata=pdfn:$*.pdf \
		--css=spec.css \
		--mathjax \
		--filter=pandoc-citeproc \
		--csl=pandoc/ieee-with-url.csl \
		-o $@

clean:
	rm -f out/radicle-link.*
