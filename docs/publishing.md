# Preview and publish the documentation

[Documentation](README.md) · [中文文档目录](README.zh-CN.md)

The project READMEs, the documentation under `docs/`, and the crate guides are
the source for the searchable MkDocs site. The build keeps documentation
links and images in the site and opens referenced Rust source on GitHub.

## Preview locally

Run these commands from the repository root in a Python 3.13 environment:

```sh
python -m pip install -r .github/pages/requirements.txt
python .github/pages/build.py
python -m http.server 8000 --bind 127.0.0.1 --directory target/pages
```

Open [the local preview](http://127.0.0.1:8000/) and rerun the build after editing
Markdown. The build validates Markdown links and heading anchors.

## Deploy to GitHub Pages

1. Merge the documentation and Pages workflow into the default branch.
2. In the repository, open **Settings → Pages**. Under **Build and deployment**,
   set **Source** to **GitHub Actions**. [GitHub Pages setup guide](https://docs.github.com/en/pages/getting-started-with-github-pages/configuring-a-publishing-source-for-your-github-pages-site)
3. Open **Actions → Documentation site → Run workflow** and select the default
   branch. The successful deployment shows the site URL in the `github-pages`
   environment and under **Settings → Pages**.

Documentation pull requests run the build and link checks. Publishing uses the
manual workflow on the default branch. Rerun it there to update the live site.
