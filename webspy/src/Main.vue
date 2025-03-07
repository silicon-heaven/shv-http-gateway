<template>
  <Toolbar style="border-radius: 0px; position: fixed; top: 0; left: 0; background:
    var(--p-primary-100); width: 100%; z-index: 1000;">
    <template #start>
      <!-- <p style="font-size: 13pt;">{{selectedPath}}</p> -->
      <span class="font-bold">{{selectedPath}}</span>
    </template>
    <template #end>
      <Button @click="logout" icon="pi pi-sign-out" label="Logout" severity="contrast"
        raised size="small" />
    </template>
  </Toolbar>
  <Splitter style=" height: 100vh;" layout="vertical" :gutter-size="2">
    <SplitterPanel :size="70" :min-size="10">
      <Splitter :gutter-size="2">
        <SplitterPanel :size="20" :min-size="5" style="overflow-y: auto; margin-top: 60px;" >
            <Tree
              :value="tree"
              @node-expand="onNodeExpand"
              @nodeSelect="onNodeSelect"
              @nodeUnselect="onNodeUnselect"
              v-model:selectionKeys="selectedKey"
              selectionMode="single"
              loadingMode="icon">
            </Tree>
        </SplitterPanel>
        <SplitterPanel :size="80" :min-size="5" style="overflow-y: auto; margin-top: 60px;" >
          <div style="margin: 1rem;">
            <DataTable
              v-if="methods.length"
              :value="methods"
              v-model:selection="selectedMethodRow"
              dataKey="name"
              selectionMode="single"
              row-hover
              size="small">
              <Column field="name" header="Name"></Column>
              <Column field="param" header="Param"></Column>
              <Column field="result" header="Result"></Column>
              <Column field="accessLevel" header="Access level"></Column>
              <Column field="signals" header="Signals">
                <template #body="slotProps">
                  {{ signalsToString(slotProps.data.signals) }}
                </template>
              </Column>
            </DataTable>
            <Card v-if="selectedMethod && selectedPath">
              <template #content>
                <div class="flex flex-column row-gap-2">
                  <Fieldset legend="Param" collapsed toggleable>
                    <Textarea v-model="methodParam" rows="5" style="resize: vertical; width: 100%;" />
                  </Fieldset>
                  <div class="flex gap-2">
                    <Button @click="onClickCallMethod" label="Call" size="small" raised />
                    <Button @click="showCurlRequest" label="cURL Request" size="small" raised />
                  </div>
                  <Fieldset legend="Output">
                    <Textarea v-model="methodsOutput" rows="5" :class="{errtext: isMethodsOutputError}"
                      style="resize: vertical; width: 100%; font-family: monospace;" />
                  </Fieldset>
                </div>
              </template>
            </Card>
          </div>
        </SplitterPanel>
      </Splitter>
    </SplitterPanel>
    <SplitterPanel :size="30" :min-size="5" style="overflow: auto" >
      <Tabs value="0">
        <TabList>
          <Tab value="0">Subscriptions</Tab>
          <Tab value="1">Notifications</Tab>
        </TabList>
        <TabPanels>
          <TabPanel value="0">
            <div class="flex gap-2">
              <InputText v-model="newSubscriptionRI" size="small" style="width:30vw;" placeholder="New Subscription RI" />
              <Button @click="addSubscription(newSubscriptionRI)" label="Add" icon="pi pi-plus" size="small" />
            </div>
            <DataTable
              v-if="subscriptions.length"
              :value="subscriptions"
              dataKey="ri"
              size="small">
              <Column headerStyle="width: 3rem">
                <template #body="{ data }">
                  <Button icon="pi pi-trash" size="small" class="p-button-danger" @click="removeSubscription(data)" />
                </template>
              </Column>
              <Column field="ri"></Column>
            </DataTable>
          </TabPanel>
          <TabPanel value="1">
            <div v-if="notifications.length">
              <Button @click="notifications.length = 0" label="Clear"
              class="p-button-danger" icon="pi pi-trash" size="small" />
              <Divider />
            </div>
            <DataTable
              v-if="notifications.length"
              :value="notifications"
              dataKey="ts"
              row-hover
              size="small">
              <Column field="ts" header="Timestamp"></Column>
              <Column field="path" header="Path"></Column>
              <Column field="signal" header="Signal"></Column>
              <Column field="param" header="Param"></Column>
            </DataTable>
          </TabPanel>
        </TabPanels>
      </Tabs>
    </SplitterPanel>
  </Splitter>
  <Toast position="bottom-left" group="bl" />
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watchEffect } from 'vue';
import router from './router';
import Tree, { type TreeSelectionKeys } from 'primevue/tree';
import type { TreeNode } from 'primevue/treenode';
import {
  Button,
  Column,
  DataTable,
  Card,
  Textarea,
  Fieldset,
  Splitter,
  SplitterPanel,
  Tabs,
  Tab,
  TabList,
  TabPanels,
  TabPanel,
  Toolbar,
  InputText,
  useToast,
  Toast,
  Divider,
} from 'primevue';
import { fromCpon } from 'libshv-js/cpon.ts';
import * as z from 'libshv-js/zod.ts';
import type { ShvMap } from 'libshv-js/rpcvalue.ts';
import fetchToCurl from 'fetch-to-curl';

const KEY_METHOD_NAME = 1;
const KEY_PARAM = 3;
const KEY_RESULT = 4;
const KEY_ACCESS_LEVEL = 5;
const KEY_SIGNALS = 6;

const DirArrayZod = z.array(z.imap({
  [KEY_METHOD_NAME]: z.string(),
  [KEY_PARAM]: z.string().optional(),
  [KEY_RESULT]: z.string().optional(),
  [KEY_ACCESS_LEVEL]: z.int().or(z.string()).optional(),
  [KEY_SIGNALS]: z.recmap(z.rpcvalue()).optional(),
}))
.transform(arg => arg.map(entry => ({
  name: entry[KEY_METHOD_NAME],
  param: entry[KEY_PARAM],
  result: entry[KEY_RESULT],
  accessLevel: entry[KEY_ACCESS_LEVEL],
  signals: entry[KEY_SIGNALS],
})));

type DirArray = z.infer<typeof DirArrayZod>;

const tree = ref<TreeNode[]>([]);
const selectedKey = ref<TreeSelectionKeys>();
const methods = ref<DirArray>([]);
const selectedPath = computed(() =>
  selectedKey.value !== undefined
    ? Object.keys(selectedKey.value)[0]
    : undefined
);
const selectedMethodRow = ref<DirArray[number]>();
const selectedMethod = computed(() => selectedMethodRow.value?.name);
const methodParam = ref<string | undefined>();
const methodsOutput = ref('');
const isMethodsOutputError = ref(false);
const callRpcUrl = "http://localhost:8000/api/rpc";

const toast = useToast();

interface Subscription {
  ri: string,
  reader: any,
};

interface Notification {
  ts: string,
  path: string,
  signal: string,
  param: string,
};

interface RpcErrorResponse {
  code: number,
  detail: string,
  shv_error: string,
};

const newSubscriptionRI = ref('');
const subscriptions = ref<Subscription[]>([]);
const notifications = ref<Notification[]>([]);

const signalsToString = (signals: ShvMap | undefined) => {
  if (!signals) {
    return ""
  }
  let res = Object.keys(signals);
  return res.length === 0 ? "" : res;
};

// const methodHasSignal = computed(() => selectedMethodRow.value?.signals !== undefined
//   ? Object.keys(selectedMethodRow.value.signals).length > 0
//   : false
// );

watchEffect(() => {
  console.log(selectedMethod.value);
  if (!selectedPath.value || !selectedMethod.value) {
    return;
  }
});

const addSubscription = async (ri: string) => {
  if (subscriptions.value.findIndex(item => item.ri === ri) !== -1) {
    toast.add({
      severity: "warn",
      summary: `${ri} is already subscribed`,
      group: 'bl',
      life: 5000,
    });
    return;
  }

  const session_id = localStorage.getItem("session_id");
  if (!session_id) {
    router.push('/login');
    return;
  }

  try {
    const response = await fetch("http://localhost:8000/api/subscribe", {
      method: 'POST',
      body: JSON.stringify({ shv_ri: ri}),
      headers: {
        'Authorization': `${session_id}`,
        'Content-Type': 'application/json',
      },
    });
    if (!response.ok) {
      const error_body: RpcErrorResponse = await response.json();
      console.error(`Cannot subscribe ${ri}, response: ${response.status}, detail: ${error_body.detail}`);
      if (response.status === 401) {
        // Unauthorized - session ID expired or invalid
        await logout();
      }
      toast.add({
        severity: "error",
        summary: `Cannot subscribe '${ri}'`,
        detail: `${response.status} ${response.statusText}\n${error_body.detail}`,
        group: 'bl',
      });
      return;
    }
    if (!response.body) {
      return;
    }
    const reader = response.body.getReader();
    const subscription: Subscription = { ri, reader };
    subscriptions.value.push(subscription);

    (async () => {
      const decoder = new TextDecoder();
      const { ri, reader } = subscription;
      while (true) {
        const { value, done } = await reader.read();
        if (done) {
          console.log(`Notification stream for '${ri}' finished`);
          removeSubscription(subscription);
          break;
        }
        const text = decoder.decode(value, { stream: true });
        // console.log(`${text}`);
        const messages = text.split("\n\n");
        for (const message of messages) {
          if (message.startsWith("data:")) {
            const jsonData = message.replace(/^data:\s*/, "");
            try {
              const notification: Notification = {
                ...JSON.parse(jsonData),
                ts: new Date().toISOString()
              };
              notifications.value.push(notification);
            } catch (err) {
              console.error("Failed to parse JSON:", err, jsonData);
            }
          }
        }
      }
    })();
  } catch (error) {
    console.error(error);
    toast.add({
      severity: "error",
      summary: `Cannot subscribe '${ri}'`,
      detail: `${error}`,
      group: 'bl',
    });
  }
};

const removeSubscription = async (row: Subscription) => {
  subscriptions.value = subscriptions.value.filter(item => {
    if (item.ri === row.ri) {
      item.reader.cancel();
      return false;
    }
    return true;
  });
};

const showCurlRequest = () => {
  const session_id = localStorage.getItem("session_id");
  if (!session_id || !selectedPath.value || !selectedMethod.value) {
    return;
  }
  const param = methodParam.value && methodParam.value.length > 0 ? methodParam.value : undefined;
  const params = callRpcMethodParams(selectedPath.value, selectedMethod.value, session_id, param);
  isMethodsOutputError.value = false;
  methodsOutput.value = fetchToCurl(callRpcUrl, params);
};

const onClickCallMethod = async () => {
  if (!selectedPath.value || !selectedMethod.value) {
    return;
  }
  const param = methodParam.value && methodParam.value.length > 0 ? methodParam.value : undefined;
  const res = await callRpcMethod(selectedPath.value, selectedMethod.value, param);
  console.log(`onClickCallMethod: ${res}`);
  if (!res) {
    return;
  }
  if (res instanceof Error) {
    isMethodsOutputError.value = true;
    methodsOutput.value = res.message;
  }
  else {
    isMethodsOutputError.value = false;
    methodsOutput.value = res;
  }
};

const callRpcMethodParams = (path: string, method: string, session_id: string, param?: string) => {
  const body: Record<string, any> = {
    path,
    method,
  }
  if (param !== undefined) {
    body.param = param;
  }
  return {
      method: 'POST',
      body: JSON.stringify(body),
      headers: {
        'Authorization': `${session_id}`,
        'Content-Type': 'application/json',
      },
    };
};

const callRpcMethod = async (path: string, method: string, param?: string) => {
  const session_id = localStorage.getItem("session_id");
  if (!session_id) {
    router.push('/login');
    return;
  }

  interface RpcResponse {
    result: string
  }

  try {
    const response = await fetch(callRpcUrl, callRpcMethodParams(path, method, session_id, param));
    if (!response.ok) {
      const error_body: RpcErrorResponse = await response.json();
      console.error(`callRpcMethod ${path}:${method}, response: ${response.status}, detail: ${error_body.detail}`);
      if (response.status === 401) {
        // Unauthorized - session ID expired or invalid
        await logout();
      }
      return new Error(error_body.detail);
    }
    const response_body: RpcResponse = await response.json();
    return response_body.result;
  } catch (error) {
    console.error(error);
    return new Error("Network error");
  }
}

const callLs = async (path: string) => {
  const response = await callRpcMethod(path, 'ls');
  if (!response || response instanceof Error) {
    return [];
  }
  try {
    const res: string[] = JSON.parse(response);
    return res ? res : [];
  } catch (error) {
    console.error(`Cannot parse ls result of ${path}`);
    return [];
  }
}

const callDir = async (path: string) => {
  const response = await callRpcMethod(path, 'dir');
  if (!response || response instanceof Error) {
    console.log("Call `dir` error");
    return [];
  }
  const parsedRes = DirArrayZod.safeParse(fromCpon(response));
  if (!parsedRes.success) {
    console.warn(fromCpon(response));
    console.log(parsedRes.error);
    return [];
  }
  return parsedRes.data;
}

const fetchNodes = async (path: string) => {
  const response: string[] = await callLs(path);
  const nodes: TreeNode[] = await Promise.all(response.map(async (child: string) => {
    const childPath = path == '' ? child : `${path}/${child}`;
    const childLsResult = await callLs(childPath);
    return {
      label: child,
      key: childPath,
      leaf: childLsResult.length == 0,
    };
  }));
  return nodes;
};

onMounted(async () => {
  tree.value = await fetchNodes('');
});

const onNodeExpand = async (node: TreeNode) => {
  node.loading = true;
  node.children = await fetchNodes(node.key);
  node.loading = false;
}

const onNodeSelect = async (node: TreeNode) => {
  methodsOutput.value = '';
  selectedMethodRow.value = undefined;
  methods.value = await callDir(node.key);
}

const onNodeUnselect = async (_node: TreeNode) => {
  methodsOutput.value = '';
  selectedMethodRow.value = undefined;
  methods.value = [];
}

const logout = async () => {
  const session_id = localStorage.getItem("session_id");
  if (session_id) {
    localStorage.removeItem("session_id");
    try {
      await fetch("http://localhost:8000/api/logout",
        {
          method: 'POST',
          headers: {
            Authorization: `${session_id}`,
          },
        }
      );
    } catch(error) {
      console.error(error);
    }
  }
  router.push('/login');
  return;
}
</script>

<style scoped>
.page {
	display: grid;
  height: 60vh;
	grid-template-columns: 2fr 5fr;
	grid-template-rows: auto ;
  align-content: start;
  gap: 1rem;
	grid-template-areas:
		"header header"
		"tree methods";
	padding: 1rem;
}

.errtext {
  color: red;
}
</style>
