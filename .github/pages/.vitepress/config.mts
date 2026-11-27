import { posix } from 'node:path'
import { defineConfig, type DefaultTheme } from 'vitepress'
import { publishedPages } from '../stage.mjs'

const repository = 'https://github.com/emojiiii/gpui-flow'
const sourceLinks = new Map([
  ['.github/workflows/ci.yml', 'blob'],
  ['.github/workflows/pages.yml', 'blob'],
  ['crates/flow-creative/src/main.rs', 'blob'],
  ['docs/examples/src/bin', 'tree'],
  ['docs/examples/src/bin/plugins.rs', 'blob'],
  ['docs/examples/src/bin/runner.rs', 'blob'],
])
const siteUrl = process.env.PAGES_SITE_URL || 'https://emojiiii.github.io/gpui-flow/'
const base = `${new URL(siteUrl).pathname.replace(/\/$/, '')}/`
const pages = await publishedPages()
const originalPaths = new Map([...pages].map(([original, staged]) => [staged, original]))
const pageLanguages = new Map([
  ['index.md', 'en'],
  ['docs/index.md', 'en'],
  ['docs/getting-started/index.md', 'en'],
  ['docs/flow-creative/index.md', 'en'],
  ['overview/index.md', 'en'],
  ['README.zh-CN/index.md', 'zh-CN'],
  ['docs/README.zh-CN/index.md', 'zh-CN'],
  ['docs/getting-started.zh-CN/index.md', 'zh-CN'],
  ['crates/flow-creative/index.md', 'zh-CN'],
  ['overview/zh/index.md', 'zh-CN'],
])

const sidebar: DefaultTheme.SidebarItem[] = [
  {
    text: 'Get started',
    items: [
      { text: 'Documentation index', link: '/docs/' },
      { text: 'Installation and first project', link: '/docs/getting-started/' },
      { text: 'Flow Creative guide', link: '/docs/flow-creative/' },
      { text: 'Project overview', link: '/overview/' },
    ],
  },
  {
    text: 'Library',
    items: [
      { text: 'Graph architecture', link: '/docs/architecture/' },
      { text: 'Custom nodes', link: '/docs/custom-nodes/' },
      { text: 'Workflows and runners', link: '/docs/workflows/' },
      { text: 'Display scenes', link: '/docs/display-scenes/' },
      { text: 'Components', link: '/docs/components/' },
      { text: 'Compatibility', link: '/docs/leafer-compatibility-profile/' },
    ],
  },
  {
    text: 'Development',
    items: [
      { text: 'Testing', link: '/docs/testing/' },
      { text: 'Known limitations', link: '/docs/limitations/' },
      { text: 'Publishing the documentation', link: '/docs/publishing/' },
    ],
  },
  {
    text: '简体中文',
    items: [
      { text: '文档目录', link: '/docs/README.zh-CN/' },
      { text: '环境准备与入门', link: '/docs/getting-started.zh-CN/' },
      { text: 'Flow Creative 操作指南', link: '/crates/flow-creative/' },
      { text: '项目概览', link: '/overview/zh/' },
    ],
  },
]

function rewriteLink(source: string, link: string) {
  if (/^(?:[\w+.-]+:|\/|#)/.test(link)) return link

  const path = link.split(/[?#]/, 1)[0]
  const suffix = link.slice(path.length)
  const resolved = posix.normalize(posix.join(posix.dirname(source), decodeURI(path))).replace(/\/$/, '')
  const page = pages.get(resolved)
  if (page) return `/${page.replace(/index\.md$/, '')}${suffix}`

  const view = sourceLinks.get(resolved)
  if (view) return `${repository}/${view}/main/${resolved}${suffix}`

  if (resolved.startsWith('docs/images/') || resolved.startsWith('docs/assets/') || resolved === 'docs/leafer-task-ledger.json') {
    return `/${resolved}${suffix}`
  }
  return link
}

export default defineConfig({
  title: 'gpui-flow',
  description: 'Documentation for Flow Creative and the Rust / GPUI canvas libraries',
  lang: 'en',
  base,
  srcDir: 'staged',
  outDir: '../../target/pages',
  cleanUrls: true,
  sitemap: { hostname: new URL(base, siteUrl).href },
  transformPageData(page) {
    const language = pageLanguages.get(page.relativePath)
    if (language) page.frontmatter.uiLanguage = language
  },
  transformHtml(html, _id, { pageData }) {
    if (pageData.frontmatter.uiLanguage === 'zh-CN') {
      return html.replace('<html lang="en"', '<html lang="zh-CN"')
    }
  },
  themeConfig: {
    nav: [
      { text: 'Documentation', link: '/docs/' },
      { text: 'Flow Creative', link: '/docs/flow-creative/' },
      { text: 'English', link: '/' },
      { text: '简体中文', link: '/README.zh-CN/' },
    ],
    sidebar,
    socialLinks: [{ icon: 'github', link: repository }],
    search: { provider: 'local' },
  },
  markdown: {
    anchor: {
      slugify: (title) => title.replace(/[^\p{L}\p{N}_\s-]/gu, '').trim().toLowerCase().replace(/[\s-]+/gu, '-'),
    },
    config(markdown) {
      markdown.core.ruler.push('repository-links', (state) => {
        const source = originalPaths.get(state.env.relativePath)
        if (!source) return
        for (const block of state.tokens) {
          for (const token of block.children ?? []) {
            const attribute = token.type === 'link_open' ? 'href' : token.type === 'image' ? 'src' : ''
            if (!attribute) continue
            const link = token.attrGet(attribute)
            if (link) token.attrSet(attribute, rewriteLink(source, link))
          }
        }
      })

      const fence = markdown.renderer.rules.fence!
      markdown.renderer.rules.fence = (tokens, index, options, env, self) => {
        const token = tokens[index]
        if (token.info.trim() === 'mermaid') {
          const expression = markdown.utils.escapeHtml(JSON.stringify(token.content))
          return `<MermaidDiagram :source="${expression}" />\n`
        }
        return fence(tokens, index, options, env, self)
      }
    },
  },
})
