import DefaultTheme from 'vitepress/theme'
import type { Theme } from 'vitepress'
import MermaidDiagram from '../../components/MermaidDiagram.vue'
import FlowLanding from './FlowLanding.vue'
import LocalizedLayout from './LocalizedLayout.vue'
import './style.css'

export default {
  extends: DefaultTheme,
  Layout: LocalizedLayout,
  enhanceApp({ app }) {
    app.component('FlowLanding', FlowLanding)
    app.component('MermaidDiagram', MermaidDiagram)
  },
} satisfies Theme
