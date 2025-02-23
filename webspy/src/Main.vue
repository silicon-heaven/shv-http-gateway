<template>
  <div class="page">
    <div style="grid-area: header; margin: 2rem; ">
      <Toolbar>
        <template #start>
          <h3>{{selectedPath}}</h3>
        </template>
        <template #end>
          <Button @click="logout" icon="pi pi-sign-out" label="Logout" severity="contrast" raised />
        </template>
      </Toolbar>
    </div>
    <div style="grid-area: tree; overflow-y: auto;">
      <Tree
        :value="tree"
        @node-expand="onNodeExpand"
        @nodeSelect="onNodeSelect"
        @nodeUnselect="onNodeUnselect"
        v-model:selectionKeys="selectedKey"
        selectionMode="single"
        loadingMode="icon">
      </Tree>
    </div>
    <div style="grid-area: methods;">
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
        <Column field="accessLevel" header="Access level"> </Column>
        <Column field="signals" header="Signals">
          <template #body="slotProps">
            {{ signalsToString(slotProps.data.signals) }}
          </template>
        </Column>
      </DataTable>
      <Card v-if="selectedMethod && selectedPath">
        <template #content>
          <Tabs value="0">
            <TabList>
              <Tab value="0">Method call</Tab>
              <!-- <Tab value="1">API request</Tab> -->
            </TabList>
            <TabPanels>
              <TabPanel value="0">
                <div class="flex flex-column row-gap-2">
                  <Fieldset legend="Param" collapsed toggleable>
                    <Textarea v-model="methodParam" rows="5" style="resize: vertical; width: 100%;" />
                  </Fieldset>
                  <Button @click="onClickCallMethod" icon="pi pi-check" label="Call" raised />
                  <Fieldset legend="Result">
                    <Textarea v-model="methodCallResult" rows="5" :class="{errtext: isMethodCallError}" style="resize: vertical; width: 100%" />
                  </Fieldset>
                </div>
              </TabPanel>
              <!-- <TabPanel value="1"> -->
              <!--   <p class="m-0">API request</p> -->
              <!-- </TabPanel> -->
            </TabPanels>
          </Tabs>
        </template>
      </Card>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watchEffect } from 'vue';
import axios from 'axios';
import router from './router';
import Tree, { type TreeSelectionKeys } from 'primevue/tree';
import type { TreeNode } from 'primevue/treenode';
import { Button, Column, DataTable, Tab, TabList, TabPanels, TabPanel, Tabs, Card, Textarea, Fieldset, Toolbar} from 'primevue';
import { fromCpon } from 'libshv-js/cpon.ts';
import * as z from 'libshv-js/zod.ts';
import type { ShvMap } from 'libshv-js/rpcvalue.ts';

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
const methodCallResult = ref('');
const isMethodCallError = ref(false);

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
});

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
    isMethodCallError.value = true;
    methodCallResult.value = res.message;
  }
  else {
    isMethodCallError.value = false;
    methodCallResult.value = res.result;
  }
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

  const body: Record<string, any> = {
    path,
    method,
  }
  if (param !== undefined) {
    body.param = param;
  }

  try {
    const response = await axios.post<RpcResponse>("http://localhost:8000/api/rpc",
      body,
      {
        headers: {
          Authorization: `${session_id}`,
          'Content-Type': 'application/json',
        },
      }
    );
    return response.data;

  } catch (error) {
    console.error(error);
    if (axios.isAxiosError(error)) {
      if (error.status === 401) {
        // Unauthorized - session ID expired or invalid
        await logout();
      }
      return new Error(error.response?.data.detail);
    }
    return undefined;
  }
}

const callLs = async (path: string) => {
  const response = await callRpcMethod(path, 'ls');
  if (!response || response instanceof Error) {
    return [];
  }
  try {
    const res: string[] = JSON.parse(response.result);
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
  const parsedRes = DirArrayZod.safeParse(fromCpon(response.result));
  if (!parsedRes.success) {
    console.warn(fromCpon(response.result));
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
  methodCallResult.value = '';
  selectedMethodRow.value = undefined;
  methods.value = await callDir(node.key);
}

const onNodeUnselect = async (_node: TreeNode) => {
  methodCallResult.value = '';
  selectedMethodRow.value = undefined;
  methods.value = [];
}

const logout = async () => {
  const session_id = localStorage.getItem("session_id");
  if (session_id) {
    localStorage.removeItem("session_id");
    try {
      await axios.post("http://localhost:8000/api/logout", null,
        {
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
  height: 100vh;
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
