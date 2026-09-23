<script setup>
import { ConfigProvider } from 'ant-design-vue';
import zhCN from 'ant-design-vue/es/locale/zh_CN';
import Header from './components/Header.vue';
import GenEntity from './views/GenEntity.vue';
import GenPages from './views/GenPages.vue';
import GenUndo from './views/GenUndo.vue';
import Doctor from './views/Doctor.vue';
import NewProject from './views/NewProject.vue';
import Adopt from './views/Adopt.vue';
import Manifest from './views/Manifest.vue';

const tabs = [
  { key: 'entity', label: '实体链', component: GenEntity },
  { key: 'pages', label: '页面生成', component: GenPages },
  { key: 'undo', label: '回滚', component: GenUndo },
  { key: 'new', label: '脚手架', component: NewProject },
  { key: 'adopt', label: '接管', component: Adopt },
  { key: 'manifest', label: '清单', component: Manifest },
  { key: 'doctor', label: '环境体检', component: Doctor },
];
</script>

<template>
  <ConfigProvider :locale="zhCN" :theme="{ token: { colorPrimary: '#00b96b' } }">
    <div class="page-container">
      <Header />
      <div class="layout-wrapper">
        <a-tabs tab-position="left" class="tabs-container">
          <a-tab-pane v-for="item in tabs" :key="item.key" :tab="item.label">
            <component :is="item.component" />
          </a-tab-pane>
        </a-tabs>
      </div>
    </div>
  </ConfigProvider>
</template>
