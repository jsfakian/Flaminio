package failover

import (
	"bytes"
	"context"
	"crypto/tls"
	"crypto/x509"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"strings"
	"time"
)

// StorageLabel selects the nodes this controller looks after.
const StorageLabel = "flaminio.io/storage=true"

// APIClient is a minimal Kubernetes API client for nodes: list, and patch taints. It uses
// the pod's service account (in-cluster), and needs get, list and patch on nodes.
type APIClient struct {
	Base   string
	Token  func() (string, error)
	Client *http.Client
}

// InCluster configures the client from the pod's service account.
func InCluster() (*APIClient, error) {
	const sa = "/var/run/secrets/kubernetes.io/serviceaccount"
	host, port := os.Getenv("KUBERNETES_SERVICE_HOST"), os.Getenv("KUBERNETES_SERVICE_PORT")
	if host == "" || port == "" {
		return nil, fmt.Errorf("not running in a Kubernetes pod")
	}
	ca, err := os.ReadFile(sa + "/ca.crt")
	if err != nil {
		return nil, err
	}
	pool := x509.NewCertPool()
	if !pool.AppendCertsFromPEM(ca) {
		return nil, fmt.Errorf("no certificate in %s/ca.crt", sa)
	}
	return &APIClient{
		Base: "https://" + strings.Trim(host, "[]") + ":" + port,
		// Projected tokens are rotated: read it for every request.
		Token: func() (string, error) {
			b, err := os.ReadFile(sa + "/token")
			return strings.TrimSpace(string(b)), err
		},
		Client: &http.Client{
			Timeout:   15 * time.Second,
			Transport: &http.Transport{TLSClientConfig: &tls.Config{RootCAs: pool, MinVersion: tls.VersionTLS12}},
		},
	}, nil
}

func (c *APIClient) do(ctx context.Context, method, path, contentType string, body []byte, out any) error {
	req, err := http.NewRequestWithContext(ctx, method, c.Base+path, bytes.NewReader(body))
	if err != nil {
		return err
	}
	tok, err := c.Token()
	if err != nil {
		return err
	}
	req.Header.Set("Authorization", "Bearer "+tok)
	req.Header.Set("Accept", "application/json")
	if contentType != "" {
		req.Header.Set("Content-Type", contentType)
	}
	resp, err := c.Client.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	b, err := io.ReadAll(io.LimitReader(resp.Body, 16<<20))
	if err != nil {
		return err
	}
	if resp.StatusCode/100 != 2 {
		return fmt.Errorf("%s %s: %s: %s", method, path, resp.Status, bytes.TrimSpace(b))
	}
	if out != nil {
		return json.Unmarshal(b, out)
	}
	return nil
}

type apiNode struct {
	Metadata struct {
		Name            string            `json:"name"`
		ResourceVersion string            `json:"resourceVersion"`
		Annotations     map[string]string `json:"annotations"`
	} `json:"metadata"`
	Spec struct {
		// Kept as they are: a patch writes back every taint it does not own.
		Taints []json.RawMessage `json:"taints"`
	} `json:"spec"`
	Status struct {
		Conditions []struct {
			Type               string    `json:"type"`
			Status             string    `json:"status"`
			LastTransitionTime time.Time `json:"lastTransitionTime"`
		} `json:"conditions"`
	} `json:"status"`
}

type taint struct {
	Key    string `json:"key"`
	Value  string `json:"value,omitempty"`
	Effect string `json:"effect"`
}

func isOurTaint(raw json.RawMessage) bool {
	var t taint
	return json.Unmarshal(raw, &t) == nil && t.Key == TaintKey && t.Effect == TaintEffect
}

func (n *apiNode) toNode() Node {
	out := Node{Name: n.Metadata.Name}
	for _, c := range n.Status.Conditions {
		if c.Type == "Ready" {
			out.Ready = c.Status == "True"
			out.Since = c.LastTransitionTime
		}
	}
	for _, t := range n.Spec.Taints {
		if isOurTaint(t) {
			out.OutOfService = true
		}
	}
	_, out.Ours = n.Metadata.Annotations[OwnerAnnotation]
	return out
}

func (c *APIClient) StorageNodes(ctx context.Context) ([]Node, error) {
	var list struct {
		Items []apiNode `json:"items"`
	}
	if err := c.do(ctx, http.MethodGet, "/api/v1/nodes?labelSelector="+url.QueryEscape(StorageLabel), "", nil, &list); err != nil {
		return nil, err
	}
	out := make([]Node, len(list.Items))
	for i := range list.Items {
		out[i] = list.Items[i].toNode()
	}
	return out, nil
}

// SetOutOfService rewrites the node's taints with a merge patch that carries the
// resourceVersion it read: if the node changed meanwhile, the API server refuses (409), and
// the next step tries again from what is current.
func (c *APIClient) SetOutOfService(ctx context.Context, name string, on bool) error {
	var n apiNode
	if err := c.do(ctx, http.MethodGet, "/api/v1/nodes/"+url.PathEscape(name), "", nil, &n); err != nil {
		return err
	}
	taints := []json.RawMessage{}
	for _, t := range n.Spec.Taints {
		if !isOurTaint(t) {
			taints = append(taints, t)
		}
	}
	var annotation any // JSON null removes it
	if on {
		b, _ := json.Marshal(taint{Key: TaintKey, Value: TaintValue, Effect: TaintEffect})
		taints = append(taints, b)
		annotation = time.Now().UTC().Format(time.RFC3339)
	}
	patch, err := json.Marshal(map[string]any{
		"metadata": map[string]any{
			"resourceVersion": n.Metadata.ResourceVersion,
			"annotations":     map[string]any{OwnerAnnotation: annotation},
		},
		"spec": map[string]any{"taints": taints},
	})
	if err != nil {
		return err
	}
	return c.do(ctx, http.MethodPatch, "/api/v1/nodes/"+url.PathEscape(name), "application/merge-patch+json", patch, nil)
}
