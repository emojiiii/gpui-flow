<script setup>
import { onMounted, ref, useId } from 'vue'

const { source } = defineProps({ source: { type: String, required: true } })
const id = `flow-diagram-${useId()}`
const graphic = ref('')

onMounted(async () => {
  const cdn = 'https://cdn.jsdelivr.net/npm/mermaid@12.3.0/dist/mermaid.esm.min.mjs'
  const { default: mermaid } = await import(/* @vite-ignore */ cdn)
  mermaid.initialize({ startOnLoad: false, securityLevel: 'strict' })
  const { svg } = await mermaid.render(id, source)
  graphic.value = svg
})
</script>

<template>
  <div v-if="graphic" class="flow-diagram" v-html="graphic"></div>
  <pre v-else>{{ source }}</pre>
</template>

<style scoped>
.flow-diagram {
  margin-block: 1rem;
  overflow: auto;
  text-align: center;
  background: white;
  border-radius: 8px;
}
</style>
