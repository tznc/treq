import {
  actionRequest,
  fetchLastFailure,
  fetchMachine,
  machineActions,
  machineStatus,
  regionLabel,
  runMachineAction,
  sizeLabel,
  type Machine,
} from './machines';
import {supabase} from './supabase';

jest.mock('./supabase', () => ({
  supabase: {functions: {invoke: jest.fn()}, from: jest.fn()},
}));

const invoke = supabase.functions.invoke as jest.Mock;
const from = supabase.from as jest.Mock;

const machine = (status: string): Machine => ({
  instance_id: '7d0f8f0e-3a52-4c55-8b1e-0e1f4b9c2a11',
  status,
  region: 'us_east',
  size_preset: 'small',
  tier_id: 's',
  created_at: '2026-11-01T10:00:00Z',
  updated_at: '2026-11-02T10:00:00Z',
});

const httpError = (status: number, body: unknown) => ({
  context: new Response(JSON.stringify(body), {status}),
});

beforeEach(() => {
  invoke.mockReset();
  from.mockReset();
});

describe('machineActions', () => {
  test('offers Create when there is no machine and the user has Pro', () => {
    expect(machineActions(null, true)).toEqual(['ensure']);
    expect(machineActions(null, false)).toEqual([]);
  });

  test('a failed machine can be retried with Pro and deleted without', () => {
    expect(machineActions(machine('failed'), true)).toEqual(['ensure', 'delete']);
    expect(machineActions(machine('failed'), false)).toEqual(['delete']);
  });

  test('a stopped machine can be started with Pro', () => {
    expect(machineActions(machine('suspended'), true)).toEqual(['wake', 'delete']);
    expect(machineActions(machine('suspended'), false)).toEqual(['delete']);
  });

  test('a running machine can only be deleted', () => {
    expect(machineActions(machine('ready'), true)).toEqual(['delete']);
  });

  test('a machine being deleted offers nothing', () => {
    expect(machineActions(machine('deleting'), true)).toEqual([]);
  });
});

describe('labels', () => {
  test('statuses read as the user sees them, with a tone', () => {
    expect(machineStatus('ready')).toEqual({label: 'Running', tone: 'ok'});
    expect(machineStatus('suspended')).toEqual({label: 'Stopped', tone: 'idle'});
    expect(machineStatus('failed')).toEqual({label: 'Failed', tone: 'error'});
    expect(machineStatus('bootstrapping')).toEqual({label: 'Starting', tone: 'busy'});
    expect(machineStatus('something_new')).toEqual({label: 'something_new', tone: 'idle'});
  });

  test('size prefers the tier and falls back to the size preset', () => {
    expect(sizeLabel(machine('ready'))).toBe('Small');
    expect(sizeLabel({...machine('ready'), tier_id: 'xl'})).toBe('Extra large');
    expect(sizeLabel({...machine('ready'), tier_id: null, size_preset: 'large'})).toBe('Large');
  });

  test('regions have names', () => {
    expect(regionLabel('eu_west')).toBe('EU West');
    expect(regionLabel('mars_1')).toBe('mars_1');
  });
});

describe('actionRequest', () => {
  test('delete and wake name the instance and carry the key', () => {
    expect(actionRequest('delete', machine('failed'), 'k1')).toEqual({
      action: 'delete',
      instance_id: machine('failed').instance_id,
      idempotency_key: 'k1',
    });
    expect(actionRequest('wake', machine('suspended'), 'k2')).toEqual({
      action: 'wake',
      instance_id: machine('suspended').instance_id,
      idempotency_key: 'k2',
    });
  });

  test('ensure needs only the key', () => {
    expect(actionRequest('ensure', null, 'k3')).toEqual({action: 'ensure', idempotency_key: 'k3'});
  });
});

describe('fetchMachine', () => {
  test('returns the instance from status', async () => {
    invoke.mockResolvedValue({data: {instance: machine('ready'), endpoint: null}, error: null});
    await expect(fetchMachine()).resolves.toEqual(machine('ready'));
    expect(invoke).toHaveBeenCalledWith('remote-instance', {body: {action: 'status'}});
  });

  test('returns null when the user has no machine', async () => {
    invoke.mockResolvedValue({data: {instance: null, endpoint: null}, error: null});
    await expect(fetchMachine()).resolves.toBeNull();
  });

  test('throws the server sentence on failure', async () => {
    invoke.mockResolvedValue({data: null, error: httpError(500, {error: 'Internal error'})});
    await expect(fetchMachine()).rejects.toThrow('Internal error');
  });
});

describe('runMachineAction', () => {
  test('sends a fresh idempotency key each time', async () => {
    invoke.mockResolvedValue({data: {status: 'succeeded'}, error: null});
    await runMachineAction('delete', machine('failed'));
    await runMachineAction('delete', machine('failed'));
    const [first, second] = invoke.mock.calls.map(([, {body}]) => body);
    expect(first.action).toBe('delete');
    expect(first.instance_id).toBe(machine('failed').instance_id);
    expect(first.idempotency_key).toMatch(/^delete-/);
    expect(first.idempotency_key).not.toBe(second.idempotency_key);
  });

  test('marks a Pro refusal', async () => {
    invoke.mockResolvedValue({
      data: null,
      error: httpError(402, {error: 'Cloud workspaces need Pro.', code: 'pro_required'}),
    });
    await expect(runMachineAction('ensure', null)).rejects.toMatchObject({
      message: 'Cloud workspaces need Pro.',
      needsPro: true,
    });
  });

  test('passes on the provider error sentence', async () => {
    invoke.mockResolvedValue({
      data: null,
      error: httpError(502, {error: 'The provider is unavailable', code: 'provider_unavailable'}),
    });
    await expect(runMachineAction('delete', machine('failed'))).rejects.toMatchObject({
      message: 'The provider is unavailable',
      needsPro: false,
    });
  });
});

describe('fetchLastFailure', () => {
  const opsQuery = (rows: unknown[]) => {
    const query: Record<string, jest.Mock> = {};
    for (const method of ['select', 'eq', 'in', 'order']) query[method] = jest.fn(() => query);
    query.limit = jest.fn(() => Promise.resolve({data: rows, error: null}));
    from.mockReturnValue(query);
    return query;
  };

  test('returns the error of the latest lifecycle operation when it failed', async () => {
    const query = opsQuery([{status: 'failed', error_message: 'provider_unavailable: no capacity'}]);
    await expect(fetchLastFailure('id-1')).resolves.toBe('provider_unavailable: no capacity');
    expect(from).toHaveBeenCalledWith('remote_instance_operations');
    expect(query.eq).toHaveBeenCalledWith('instance_id', 'id-1');
  });

  test('returns null once a later operation succeeded', async () => {
    opsQuery([{status: 'succeeded', error_message: null}]);
    await expect(fetchLastFailure('id-1')).resolves.toBeNull();
  });
});
