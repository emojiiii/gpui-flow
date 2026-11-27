import { copyFile, mkdir, readdir, readFile, rm, writeFile } from 'node:fs/promises'
import { dirname, extname, join, posix } from 'node:path'
import { fileURLToPath } from 'node:url'

const project = fileURLToPath(new URL('./', import.meta.url))
const repository = fileURLToPath(new URL('../../', import.meta.url))
const staged = join(project, 'staged')
const media = new Set(['.avif', '.gif', '.jpeg', '.jpg', '.png', '.svg', '.webp'])

export async function publishedPages() {
  const pages = new Map([
    ['README.md', 'overview/index.md'],
    ['README.zh-CN.md', 'overview/zh/index.md'],
    ['vendor/gpui_wgpu/PATCH.md', 'vendor/gpui_wgpu/PATCH/index.md'],
    ['vendor/gpui_wgpu/SOURCE.md', 'vendor/gpui_wgpu/SOURCE/index.md'],
  ])

  for (const directory of await readdir(join(repository, 'crates'), { withFileTypes: true })) {
    if (!directory.isDirectory() || !directory.name.startsWith('flow-')) continue
    const crate = `crates/${directory.name}`
    const entries = await readdir(join(repository, crate))
    if (entries.includes('README.md')) pages.set(`${crate}/README.md`, `${crate}/index.md`)
  }

  for (const file of await readdir(join(repository, 'docs'), { withFileTypes: true })) {
    if (!file.isFile() || !file.name.endsWith('.md')) continue
    const name = file.name.slice(0, -3)
    const page = name === 'README' ? 'docs/index.md' : `docs/${name}/index.md`
    pages.set(`docs/${file.name}`, page)
  }
  return pages
}

async function copyTo(source, destination) {
  await mkdir(dirname(destination), { recursive: true })
  await copyFile(source, destination)
}

async function copyMedia(directory) {
  for (const entry of await readdir(join(repository, directory), { withFileTypes: true })) {
    const path = posix.join(directory, entry.name)
    if (entry.isDirectory()) {
      await copyMedia(path)
    } else if (entry.isFile() && media.has(extname(entry.name).toLowerCase())) {
      await copyTo(join(repository, path), join(staged, 'public', path))
    }
  }
}

async function stage() {
  await rm(staged, { recursive: true, force: true })
  for (const [source, route] of await publishedPages()) {
    const destination = join(staged, route)
    const markdown = await readFile(join(repository, source), 'utf8')
    await mkdir(dirname(destination), { recursive: true })
    // Rustdoc's snippet options have no effect on the rendered code block.
    await writeFile(destination, markdown.replace(/^(```rust),ignore\r?$/gm, '$1'))
  }

  await copyTo(join(project, 'home/index.md'), join(staged, 'index.md'))
  await copyTo(join(project, 'home/zh.md'), join(staged, 'README.zh-CN/index.md'))
  await copyMedia('docs/images')
  const docsDirectories = await readdir(join(repository, 'docs'))
  if (docsDirectories.includes('assets')) await copyMedia('docs/assets')
  const ledger = 'docs/leafer-task-ledger.json'
  await copyTo(join(repository, ledger), join(staged, 'public', ledger))
}

if (process.argv[1] === fileURLToPath(import.meta.url)) await stage()
