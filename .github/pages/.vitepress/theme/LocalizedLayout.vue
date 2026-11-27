<script setup lang="ts">
import { dataSymbol, useData, withBase, type DefaultTheme } from 'vitepress'
import VitePressTheme from 'vitepress/theme'
import { computed, onMounted, provide, ref, watchEffect } from 'vue'

const navigation = {
  en: {
    nav: [
      { text: 'gpui-flow', link: '/docs/library/' },
      { text: 'Flow Creative', link: '/docs/flow-creative/' },
    ],
    creative: [
      {
        text: 'Flow Creative',
        items: [
          { text: 'Quickstart', link: '/docs/getting-started/' },
          { text: 'Workbench guide', link: '/docs/flow-creative/' },
          { text: 'Workbench reference', link: '/docs/creative-reference.en/' },
        ],
      },
    ],
    library: [
      {
        text: 'gpui-flow',
        items: [
          { text: 'Guide', link: '/docs/library/' },
          { text: 'Runnable examples', link: '/docs/library/#run-examples-without-a-desktop-session' },
          { text: 'Documentation index', link: '/docs/' },
        ],
      },
      {
        text: 'Reference',
        items: [
          { text: 'Graph architecture', link: '/docs/architecture.en/' },
          { text: 'Custom nodes', link: '/docs/custom-nodes.en/' },
          { text: 'Workflows and runners', link: '/docs/workflows.en/' },
          { text: 'Display scenes', link: '/docs/display-scenes.en/' },
          { text: 'Components', link: '/docs/components.en/' },
          { text: 'Compatibility', link: '/docs/leafer-compatibility-profile/' },
        ],
      },
      {
        text: 'Development',
        items: [
          { text: 'Testing', link: '/docs/testing.en/' },
          { text: 'Known limitations', link: '/docs/limitations.en/' },
          { text: 'Publishing the documentation', link: '/docs/publishing/' },
        ],
      },
    ],
  },
  zh: {
    nav: [
      { text: 'gpui-flow', link: '/docs/library.zh-CN/' },
      { text: 'Flow Creative', link: '/docs/flow-creative.zh-CN/' },
    ],
    creative: [
      {
        text: 'Flow Creative',
        items: [
          { text: '快速上手', link: '/docs/getting-started.zh-CN/' },
          { text: '工作台指南', link: '/docs/flow-creative.zh-CN/' },
          { text: '应用详细参考', link: '/crates/flow-creative/' },
        ],
      },
    ],
    library: [
      {
        text: 'gpui-flow',
        items: [
          { text: '开发指南', link: '/docs/library.zh-CN/' },
          { text: '可运行示例', link: '/docs/library.zh-CN/#在无桌面环境运行示例' },
          { text: '文档目录', link: '/docs/README.zh-CN/' },
        ],
      },
      {
        text: '参考',
        items: [
          { text: '图架构', link: '/docs/architecture/' },
          { text: '自定义节点', link: '/docs/custom-nodes/' },
          { text: '工作流与运行时', link: '/docs/workflows/' },
          { text: '显示场景', link: '/docs/display-scenes/' },
          { text: '组件', link: '/docs/components/' },
          { text: '兼容性（英文）', link: '/docs/leafer-compatibility-profile/' },
        ],
      },
      {
        text: '开发与维护',
        items: [
          { text: '测试', link: '/docs/testing/' },
          { text: '已知限制', link: '/docs/limitations/' },
          { text: '发布文档（英文）', link: '/docs/publishing/' },
        ],
      },
    ],
  },
}

const chineseTheme = {
  outline: { label: '本页目录' },
  docFooter: { prev: '上一页', next: '下一页' },
  darkModeSwitchLabel: '外观',
  darkModeSwitchTitle: '切换到深色模式',
  lightModeSwitchTitle: '切换到浅色模式',
  sidebarMenuLabel: '菜单',
  returnToTopLabel: '返回顶部',
  skipToContentLabel: '跳转到正文',
} satisfies DefaultTheme.Config

const chineseSearchTranslations = {
  button: { buttonText: '搜索', buttonAriaLabel: '搜索文档' },
  modal: {
    displayDetails: '显示详细列表',
    resetButtonTitle: '清空搜索',
    backButtonTitle: '关闭搜索',
    noResultsText: '未找到匹配结果',
    footer: {
      selectText: '选择',
      selectKeyAriaLabel: '回车键',
      navigateText: '切换选项',
      navigateUpKeyAriaLabel: '上方向键',
      navigateDownKeyAriaLabel: '下方向键',
      closeText: '关闭',
      closeKeyAriaLabel: 'Esc 键',
    },
  },
}

type DocumentationTheme = DefaultTheme.Config & { searchRoutes: { en: string[], zh: string[] } }
const data = useData<DocumentationTheme>()
const storageKey = 'gpui-flow:docs-language'
const rememberedLanguage = ref<'en' | 'zh-CN'>('en')
const language = computed(() => data.frontmatter.value.uiLanguage ?? rememberedLanguage.value)
const theme = computed<DefaultTheme.Config>(() => {
  const isChinese = language.value === 'zh-CN'
  const menu = isChinese ? navigation.zh : navigation.en
  const { docsSection, languageLinks = { en: '/docs/library/', zh: '/docs/library.zh-CN/' } } = data.frontmatter.value
  const localized = isChinese ? { ...data.theme.value, ...chineseTheme } : data.theme.value
  const { search } = localized
  const otherLanguage = isChinese ? localized.searchRoutes.en : localized.searchRoutes.zh
  const excludedSearchRoutes = new Set(otherLanguage.map(withBase))
  return {
    ...localized,
    sidebar: docsSection === 'creative' ? menu.creative : menu.library,
    nav: [
      ...menu.nav,
      {
        text: isChinese ? '简体中文' : 'English',
        items: [
          { text: 'English', link: languageLinks.en },
          { text: '简体中文', link: languageLinks.zh },
        ],
      },
    ],
    search: search?.provider === 'local'
      ? {
          ...search,
          options: {
            ...search.options,
            ...(isChinese ? { translations: chineseSearchTranslations } : {}),
            miniSearch: {
              ...search.options?.miniSearch,
              searchOptions: {
                ...search.options?.miniSearch?.searchOptions,
                filter: ({ id }) => !excludedSearchRoutes.has(id.split('#', 1)[0]),
              },
            },
          },
        }
      : search,
  }
})

// Both languages use the same VitePress index and filter results by route.
provide(dataSymbol, { ...data, lang: language, theme })

onMounted(() => {
  rememberedLanguage.value = sessionStorage.getItem(storageKey) === 'zh-CN' ? 'zh-CN' : 'en'
  watchEffect(() => {
    rememberedLanguage.value = language.value
    sessionStorage.setItem(storageKey, language.value)
    document.documentElement.lang = language.value
  }, { flush: 'post' })
})
</script>

<template>
  <VitePressTheme.Layout />
</template>
