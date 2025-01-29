import { createApp } from 'vue'
import App from './App.vue'
import router from './router'
import PrimeVue from 'primevue/config'
import Aura from '@primevue/themes/aura'
import './assets/main.css'
import 'primeicons/primeicons.css'
import 'primeflex/primeflex.css'

createApp(App)
  .use(PrimeVue,
    {
      theme: {
        preset: Aura,
        options: {
          prefix: 'p',
          darkModeSelector: 'system',
          cssLayer: false
        }
      }
    }
  )
  .use(router)
  .mount('#app')
