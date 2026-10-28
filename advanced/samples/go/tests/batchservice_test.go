package tests

import (
	"context"
	"testing"

	"github.com/nexus-rpc/sdk-go/nexus"
	"github.com/stretchr/testify/require"
	"go.temporal.io/sdk/testsuite"
	"go.temporal.io/sdk/workflow"

	"go.temporal.io/sdk/advanced/samples/go/batchservice"
)

func TestAddItemsWrapsEachValueAndReturnsOneField(t *testing.T) {
	var suite testsuite.WorkflowTestSuite
	env := suite.NewTestWorkflowEnvironment()
	var inputs []any
	lastItemID := "i2"
	responses := []batchservice.AddItemsResponse{{LastItemID: &lastItemID, Count: 2}, {}}
	addItems := nexus.NewSyncOperation("AddItems",
		func(ctx context.Context, input any, opts nexus.StartOperationOptions) (batchservice.AddItemsResponse, error) {
			inputs = append(inputs, input)
			response := responses[0]
			responses = responses[1:]
			return response, nil
		})
	service := nexus.NewService("BatchService")
	require.NoError(t, service.Register(addItems))
	env.RegisterNexusService(service)

	env.ExecuteWorkflow(func(ctx workflow.Context) ([]string, error) {
		batch := batchservice.NewBatch("b1")
		var added, empty string
		if err := batch.AddItems(ctx, "x", "y").Get(ctx, &added); err != nil {
			return nil, err
		}
		if err := batch.AddItems(ctx).Get(ctx, &empty); err != nil {
			return nil, err
		}
		return []string{added, empty}, nil
	})

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	var results []string
	require.NoError(t, env.GetWorkflowResult(&results))
	// Go answers the zero value for an unset field.
	require.Equal(t, []string{"i2", ""}, results)
	require.Equal(t, "b1", stringField(inputs[0], "BatchID"))
	items, _ := field(inputs[0], "Items").([]any)
	require.Len(t, items, 2)
	require.Equal(t, "x", stringField(items[0], "Value"))
	require.Equal(t, "y", stringField(items[1], "Value"))
}
