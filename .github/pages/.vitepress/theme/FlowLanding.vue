<script setup>
import { useData, withBase } from 'vitepress'

const { frontmatter } = useData()
</script>

<template>
  <div class="flow-home" :lang="frontmatter.lang">
    <section class="flow-hero" aria-labelledby="flow-hero-title">
      <div class="flow-hero-inner">
        <div class="flow-hero-copy">
          <p class="flow-platform">{{ frontmatter.landing.platform }}</p>
          <h1 id="flow-hero-title">
            {{ frontmatter.landing.heading }}
            <span>{{ frontmatter.landing.accent }}</span>
          </h1>
          <p class="flow-intro">{{ frontmatter.landing.intro }}</p>
          <div class="flow-actions">
            <a class="flow-button flow-button-primary" :href="withBase(frontmatter.landing.primary.link)">
              {{ frontmatter.landing.primary.text }} <span aria-hidden="true">→</span>
            </a>
            <a class="flow-button" :href="withBase(frontmatter.landing.secondary.link)">
              {{ frontmatter.landing.secondary.text }}
            </a>
          </div>
        </div>

        <figure class="flow-preview">
          <div class="flow-preview-frame">
            <img
              :src="withBase('/docs/images/flow-creative-canvas.png')"
              :alt="frontmatter.landing.imageAlt"
              width="1400"
              height="900"
              fetchpriority="high"
            >
          </div>
          <figcaption>{{ frontmatter.landing.imageCaption }}</figcaption>
        </figure>
      </div>
    </section>

    <section class="flow-guides" aria-labelledby="flow-guides-title">
      <h2 id="flow-guides-title">{{ frontmatter.landing.guidesTitle }}</h2>
      <ul class="flow-guide-list">
        <li v-for="guide in frontmatter.landing.guides" :key="guide.link">
          <a class="flow-guide" :href="withBase(guide.link)">
            <span class="flow-guide-title">
              {{ guide.title }} <span class="flow-guide-arrow" aria-hidden="true">↗</span>
            </span>
            <span class="flow-guide-detail">{{ guide.detail }}</span>
          </a>
        </li>
      </ul>
    </section>
  </div>
</template>
