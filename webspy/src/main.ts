import { createApp } from 'vue'
import App from './App.vue'
import router from './router'
import PrimeVue from 'primevue/config'
import Aura from '@primevue/themes/aura'
import './assets/main.css'
import 'primeicons/primeicons.css'
import 'primeflex/primeflex.css'
import { definePreset } from '@primevue/themes'

const presetAuraSlate = definePreset(Aura, {
  semantic: {
    primary: {
      50: '{gray.50}',
      100: '{gray.100}',
      200: '{gray.200}',
      300: '{gray.300}',
      400: '{gray.400}',
      500: '{gray.500}',
      600: '{gray.600}',
      700: '{gray.700}',
      800: '{gray.800}',
      900: '{gray.900}',
      950: '{gray.950}'
    }
  }
});

createApp(App)
  .use(PrimeVue,
    {
      theme: {
        preset: presetAuraSlate,
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
