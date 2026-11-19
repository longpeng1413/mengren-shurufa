package app.qingjian.inputmethod.clipboard.files

import android.content.Context
import android.net.*
import app.qingjian.inputmethod.core.Native
import java.net.Inet4Address
import org.json.JSONArray
import org.json.JSONObject

/** 只公布当前默认物理网络及内核路由，VPN、蜂窝、无网络均立即撤销。 */
internal object LanNetwork {
    private var manager: ConnectivityManager? = null
    private val callback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) = refresh()
        override fun onLost(network: Network) = refresh()
        override fun onLinkPropertiesChanged(network: Network, properties: LinkProperties) = refresh()
        override fun onCapabilitiesChanged(network: Network, caps: NetworkCapabilities) = refresh()
    }

    @Synchronized fun start(context: Context) {
        if (manager != null) return
        manager = context.getSystemService(ConnectivityManager::class.java)
        runCatching { manager?.registerDefaultNetworkCallback(callback) }.onFailure { manager = null }
        refresh()
    }

    @Synchronized fun stop() {
        runCatching { manager?.unregisterNetworkCallback(callback) }
        manager = null
        Native.lanFileNetwork("")
    }

    @Synchronized fun refresh() {
        Native.lanFileNetwork(runCatching { snapshot() }.getOrNull().orEmpty())
    }

    private fun snapshot(): String? {
        val cm = manager ?: return null
        val active = cm.activeNetwork ?: return null
        val caps = cm.getNetworkCapabilities(active) ?: return null
        if (caps.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR)) return null
        fun physical(network: Network): Boolean {
            val candidate = cm.getNetworkCapabilities(network) ?: return false
            return candidate.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_VPN) &&
                !candidate.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR) &&
                (candidate.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) ||
                    candidate.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET))
        }
        val net = if (physical(active)) active else {
            if (!caps.hasTransport(NetworkCapabilities.TRANSPORT_VPN)) return null
            // VPN 仍须证明底层确是物理 LAN；选中的套接字单独交给该 Network，由内核决定是否允许。
            if (!caps.hasTransport(NetworkCapabilities.TRANSPORT_WIFI) &&
                !caps.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)) return null
            @Suppress("DEPRECATION")
            cm.allNetworks.singleOrNull(::physical) ?: return null
        }
        val link = cm.getLinkProperties(net) ?: return null
        val routes = JSONArray()
        link.routes.filter { it.destination.address is Inet4Address }.forEach {
            routes.put(JSONArray().put(it.destination.address.hostAddress).put(it.destination.prefixLength)
                .put(it.`interface` == link.interfaceName && (it.gateway == null || it.gateway!!.isAnyLocalAddress)))
        }
        return JSONObject().put("handle",net.networkHandle).put("local",JSONArray(link.linkAddresses.map { it.address }
            .filterIsInstance<Inet4Address>().map { it.hostAddress })).put("routes",routes).toString()
    }
}
