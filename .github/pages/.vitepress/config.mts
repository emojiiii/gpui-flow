import { posix } from 'node:path'
import { defineConfig } from 'vitepress'
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
const translatedRoutes = [
  ['/', '/README.zh-CN/'],
  ['/docs/', '/docs/README.zh-CN/'],
  ['/docs/library/', '/docs/library.zh-CN/'],
  ['/overview/', '/overview/zh/'],
  ['/docs/getting-started/', '/docs/getting-started.zh-CN/'],
  ['/docs/flow-creative/', '/docs/flow-creative.zh-CN/'],
  ['/docs/creative-reference.en/', '/crates/flow-creative/'],
  ['/docs/architecture.en/', '/docs/architecture/'],
  ['/docs/custom-nodes.en/', '/docs/custom-nodes/'],
  ['/docs/workflows.en/', '/docs/workflows/'],
  ['/docs/display-scenes.en/', '/docs/display-scenes/'],
  ['/docs/components.en/', '/docs/components/'],
  ['/docs/testing.en/', '/docs/testing/'],
  ['/docs/limitations.en/', '/docs/limitations/'],
] as const
const chineseOnlyRoutes = [
  '/docs/animation/',
  '/docs/engine-contract-plan/',
  '/docs/engine/',
  '/docs/gradients/',
  '/docs/interaction-qa/',
  '/docs/keyboard-routing/',
  '/docs/leafer-task-ledger/',
  '/docs/media/',
  '/docs/plugins/',
  '/docs/query-indexes/',
  '/docs/runtime/',
  '/docs/source-only-status/',
  '/docs/studio-retirement/',
]
const creativeRoutes = new Set([
  '/docs/getting-started/',
  '/docs/getting-started.zh-CN/',
  '/docs/flow-creative/',
  '/docs/flow-creative.zh-CN/',
  '/docs/creative-reference.en/',
  '/crates/flow-creative/',
])

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
    const route = `/${page.relativePath.replace(/index\.md$/, '')}`
    if (creativeRoutes.has(route)) page.frontmatter.docsSection = 'creative'

    const translation = translatedRoutes.find(([en, zh]) => route === en || route === zh)
    if (translation) {
      const [en, zh] = translation
      page.frontmatter.uiLanguage = route === zh ? 'zh-CN' : 'en'
      page.frontmatter.languageLinks = { en, zh }
    } else if (chineseOnlyRoutes.includes(route)) {
      page.frontmatter.uiLanguage = 'zh-CN'
    }
  },
  transformHtml(html, _id, { pageData }) {
    if (pageData.frontmatter.uiLanguage === 'zh-CN') {
      return html.replace('<html lang="en"', '<html lang="zh-CN"')
    }
  },
  themeConfig: {
    socialLinks: [{ icon: 'github', link: repository }],
    searchRoutes: {
      en: translatedRoutes.map(([en]) => en),
      zh: [...translatedRoutes.map(([, zh]) => zh), ...chineseOnlyRoutes],
    },
    search: {
      provider: 'local',
      options: {
        miniSearch: {
          options: {
            // VitePress serializes this function for the browser, so it has no closure dependencies.
            tokenize(text, fieldName) {
              const tokens = text.split(/[\n\r\p{Z}\p{P}]+/u)
              if (!/\p{Script=Han}/u.test(text)) return tokens

              const segmenter = new Intl.Segmenter('zh-CN', { granularity: 'word' })
              return tokens.flatMap((token) => {
                if (!/\p{Script=Han}/u.test(token)) return [token]
                if (!fieldName && /^\p{Script=Han}{2,4}$/u.test(token)) return [token]

                const words = new Set([token])
                for (const { segment, isWordLike } of segmenter.segment(token)) {
                  if (isWordLike && /\p{Script=Han}/u.test(segment)) words.add(segment)
                }

                // Index short phrases directly so context-dependent word boundaries cannot hide them.
                if (fieldName) {
                  for (const [run] of token.matchAll(/\p{Script=Han}{2,}/gu)) {
                    const characters = [...run]
                    for (let start = 0; start < characters.length - 1; start++) {
                      for (let length = 2; length <= 4 && start + length <= characters.length; length++) {
                        words.add(characters.slice(start, start + length).join(''))
                      }
                    }
                  }
                }
                return [...words]
              })
            },
          },
        },
      },
    },
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
