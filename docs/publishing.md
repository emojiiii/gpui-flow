# Preview and publish the documentation

[Documentation](README.md) · [中文文档目录](README.zh-CN.md)

The searchable VitePress site uses the project READMEs, the documentation under
`docs/`, and the crate guides. The landing pages and theme live in `.github/pages/`.
The build keeps documentation links and images in the site and opens referenced
Rust source on GitHub.

## Preview locally

Run these commands from the repository root with Node.js 22.12 or later.

```sh
npm ci --prefix .github/pages
npm run build --prefix .github/pages
npm run preview --prefix .github/pages
```

Open [the local preview](http://127.0.0.1:4173/gpui-flow/). Run the build again
after editing Markdown to regenerate the site in `target/pages`. Use
`npm run dev --prefix .github/pages` to work on the VitePress theme.

## Deploy to GitHub Pages

1. Merge the documentation and Pages workflow into the default branch.
2. In the repository, open **Settings → Pages**. Under **Build and deployment**,
   set **Source** to **GitHub Actions**. [GitHub Pages setup guide](https://docs.github.com/en/pages/getting-started-with-github-pages/configuring-a-publishing-source-for-your-github-pages-site)
3. Open **Actions → Documentation site → Run workflow** and select the default
   branch. The successful deployment shows the site URL in the `github-pages`
   environment and under **Settings → Pages**.

Documentation pull requests run the build and link checks. Publishing uses the
manual workflow on the default branch. Rerun it there to update the live site.
