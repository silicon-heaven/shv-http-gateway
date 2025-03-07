<template>
  <div class="login-container">
    <Card class="border-1 border-round-xl">
      <template #title><div class="font-bold text-center">WebSpy</div></template>
      <template #content>
        <div class="flex flex-column row-gap-2">
          <InputGroup>
            <InputGroupAddon>
              <i class="pi pi-user"></i>
            </InputGroupAddon>
            <InputText v-model="username" placeholder="Username" />
          </InputGroup>
          <InputGroup>
            <InputGroupAddon>
              <i class="pi pi-lock"></i>
            </InputGroupAddon>
            <InputText type="password" v-model="password" placeholder="Password" />
          </InputGroup>
          <Button type="submit" :disabled="isLoginDisabled" @click="login" icon="pi pi-sign-in" label="Login" raised />
          <Message severity="error" icon="pi pi-times-circle" v-if="errorMessage">{{ errorMessage }}</Message>
        </div>
      </template>
    </Card>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import { useRouter } from "vue-router";
import { Button, InputText, InputGroup, InputGroupAddon, Message} from "primevue";
import Card from "primevue/card";

const router = useRouter();
const username = ref("");
const password = ref("");
const errorMessage = ref("");
const isLoginDisabled = computed(() => loading.value || !username.value || !password.value);
const loading = ref(false);

interface LoginResponse {
  session_id: string
}

interface ErrorResponse {
  code: number,
  detail: string,
}

const login = async () => {
  errorMessage.value = "";
  loading.value = true;
  try {
    const response = await fetch("http://localhost:8000/api/login", {
      method: 'POST',
      body: JSON.stringify({
        username: username.value,
        password: password.value,
      })
    });
    if (!response.ok) {
      const error_body: ErrorResponse = await response.json();
      errorMessage.value = error_body.detail;
      return;
    }
    const login_response: LoginResponse = await response.json();
    const session_id = login_response.session_id;
    localStorage.setItem("session_id", session_id);
    router.push("/main");
  } catch (error) {
    if (error instanceof Error) {
      errorMessage.value = `Log-in failed: ${error.message}`;
    } else {
      errorMessage.value = `Log-in failed: ${error}`;
    }
  } finally {
    loading.value = false;
  }
};
</script>

<style scoped>
.login-container {
  display: grid;
  place-items: center;
  height: 100vh;
}
</style>
