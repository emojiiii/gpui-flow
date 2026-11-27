<script setup lang="ts">
import { dataSymbol, useData, type DefaultTheme } from 'vitepress'
import VitePressTheme from 'vitepress/theme'
import { computed, onMounted, provide, ref, watchEffect } from 'vue'

const chineseTheme = {
  nav: [
    { text: '文档', link: '/docs/README.zh-CN/' },
    { text: 'Flow Creative 指南', link: '/crates/flow-creative/' },
    { text: 'English', link: '/' },
    { text: '简体中文', link: '/README.zh-CN/' },
  ],
  sidebar: [
    {
      text: '入门',
      items: [
        { text: '文档目录', link: '/docs/README.zh-CN/' },
        { text: '环境准备与入门', link: '/docs/getting-started.zh-CN/' },
        { text: 'Flow Creative 操作指南', link: '/crates/flow-creative/' },
        { text: '项目概览', link: '/overview/zh/' },
      ],
    },
    {
      text: '库与宿主集成',
      items: [
        { text: '图架构', link: '/docs/architecture/' },
        { text: '自定义节点', link: '/docs/custom-nodes/' },
        { text: '工作流与运行时', link: '/docs/workflows/' },
        { text: '显示场景', link: '/docs/display-scenes/' },
        { text: '组件', link: '/docs/components/' },
        { text: '兼容性', link: '/docs/leafer-compatibility-profile/' },
      ],
    },
    {
      text: '开发与维护',
      items: [
        { text: '测试', link: '/docs/testing/' },
        { text: '已知限制', link: '/docs/limitations/' },
        { text: '发布文档', link: '/docs/publishing/' },
      ],
    },
  ],
  outline: { label: '本页目录' },
  docFooter: { prev: '上一页', next: '下一页' },
  darkModeSwitchLabel: '外观',
  darkModeSwitchTitle: '切换到深色模式',
  lightModeSwitchTitle: '切换到浅色模式',
  sidebarMenuLabel: '菜单',
  returnToTopLabel: '返回顶部',
  skipToContentLabel: '跳转到正文',
  search: {
    provider: 'local',
    options: {
      translations: {
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
      },
    },
  },
} satisfies DefaultTheme.Config

const data = useData<DefaultTheme.Config>()
const storageKey = 'gpui-flow:docs-language'
const rememberedLanguage = ref<'en' | 'zh-CN'>('en')
const language = computed(() => data.frontmatter.value.uiLanguage ?? rememberedLanguage.value)
const theme = computed(() => language.value === 'zh-CN'
  ? { ...data.theme.value, ...chineseTheme }
  : data.theme.value)

// Keep VitePress's single local search index across both interface languages.
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
