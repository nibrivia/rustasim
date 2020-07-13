library(tidyverse)

dta <- read_csv("out.log", col_types = "nniiic") %>%
    filter(tx_time != 0) %>%
    mutate(sim_time = sim_time/1000,
           rx_time = (rx_time)/1000000,
           tx_time = (tx_time)/1000000,
           src = as.factor(src),
           id  = as.factor(id),
           start = ifelse(type != "ModelEvent(Packet)", sim_time-.1, sim_time-.1-1.5))

dta %>%
    group_by(type, id) %>%
    summarize(count = n()) %>%
    ggplot(aes(x = type,
               y = count,
               fill = type)) +
    geom_col() +
    facet_wrap(~id)

start_t <- round(runif(n = 1, min = 0, max = max(dta$sim_time)))
#start_t <- 20
duration <- 250
end <- start_t + duration
#end <- 450

#start_sim <- round(runif(n = 1, min = 0, max = max(dta$sim_time)))
start_sim <- 000
end_sim <- start_sim + 50
#end_sim <- max(dta$sim_time)

dta %>%
    filter(src != 0) %>%
    filter(sim_time >= start_sim, sim_time <= end_sim) %>%
    #filter(tx_time > start_t | rx_time > start_t,
           #tx_time < end | rx_time < end) %>%
    sample_n(min(100000, nrow(.))) %>%
    #filter(type == "Null") %>% #| type == "Stalled") %>%
    ggplot(aes(x = tx_time,
               y = start,
               xend = rx_time,
               yend = sim_time,
               color = type)) +
    # geom_step(aes(x = tx_time,
    #               y = start,
    #               group = paste(src)),
    #           color = "orange") +
    geom_step(aes(x = rx_time,
                  y = sim_time,
                  group = id),
              color = "black") +
    geom_segment(arrow = arrow(length = unit(.05, "inches")),
                 size = .5) +
    #coord_cartesian(xlim = c(start_t, end)) +
    labs(x = "Real time (ms)",
         y = "Simulation time (us)") +
    #geom_point() +
    facet_wrap(~id) +
    hrbrthemes::theme_ipsum_rc()

dta %>%
    sample_n(100000) %>%
    ggplot(aes(x = tx_time,
               y = rx_time,
               color = id,
               group = paste(src, id))) +
    geom_step()

dta %>%
    sample_n(100000) %>%
    #filter(time < end) %>%
    ggplot(aes(x = real_time,
               y = sim_time,
               group = src,
               color = type)) +
    geom_step() +
    labs(y = "Sim time (us)",
         x = "Real time",
         color = "Event dest")
    #geom_point(size = .3)
